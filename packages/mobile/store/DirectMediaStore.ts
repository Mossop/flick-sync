import {
  FileInfo,
  StorageAccessFramework,
  getInfoAsync,
} from "expo-file-system/legacy";
import AsyncStorage from "@react-native-async-storage/async-storage";
import { PlaybackState, PlaybackUpdates, State } from "../state/base";
import { StateDecoder } from "../state";
import { PlaybackUpdatesDecoder } from "../state/decoders";
import {
  Server,
  Library,
  Collection,
  Show,
  Playlist,
  Video,
  MediaState,
} from "../state/wrappers";
import { MediaStore } from "./MediaStore";

const STORE_KEY = "store";
const STATE_FILE = ".flicksync.state.json";
const STATE_BACKUP_FILE = ".flicksync.state.json.backup";
const PLAYBACK_FILE = ".flicksync.playback.json";
const CONTENT_ROOT = "content://com.android.externalstorage.documents/tree/";

function storagePath(store: string, path: string): string {
  let prefix = "/document";
  if (store.startsWith(CONTENT_ROOT)) {
    prefix += `/${store.substring(CONTENT_ROOT.length)}`;
  }

  return `${store}${prefix}${encodeURIComponent(`/${path}`)}`;
}

function extractPlaybackUpdates(state: State): PlaybackUpdates {
  let servers: Record<string, Record<string, PlaybackState>> = {};

  for (let [serverId, server] of Object.entries(state.servers ?? {})) {
    let videos: Record<string, PlaybackState> = {};

    for (let [videoId, video] of Object.entries(server.videos ?? {})) {
      videos[videoId] = video.playbackState;
    }

    if (Object.keys(videos).length > 0) {
      servers[serverId] = videos;
    }
  }
  return { servers };
}

async function safeInfo(path: string): Promise<FileInfo | undefined> {
  try {
    return await getInfoAsync(path);
  } catch (e) {
    console.warn(`Failed to get file metadata for ${path}`, e);
    return undefined;
  }
}

class StatePersister {
  private playbackToPersist: PlaybackUpdates | undefined = undefined;

  private isPersisting = false;

  constructor(private store: string) {}

  public async persistPlayback(state: State) {
    if (Object.keys(state.servers ?? {}).length === 0) {
      console.warn("Refusing to persist playback for empty state");
      return;
    }

    this.playbackToPersist = extractPlaybackUpdates(state);

    if (this.isPersisting) {
      return;
    }

    this.isPersisting = true;
    try {
      while (this.playbackToPersist !== undefined) {
        let path = storagePath(this.store, PLAYBACK_FILE);
        let info = await safeInfo(path);

        let writingUpdates = this.playbackToPersist;
        this.playbackToPersist = undefined;

        let data = JSON.stringify(writingUpdates, undefined, 2);

        if (!info?.exists) {
          await StorageAccessFramework.createFileAsync(
            this.store,
            PLAYBACK_FILE.substring(0, PLAYBACK_FILE.length - 5),
            "application/json",
          );
        } else if (data.length < info.size) {
          // Writes to existing files don't truncate the file so pad out the
          // data to write to the current size of the file.
          data += " ".repeat(info.size - data.length);
        }

        await StorageAccessFramework.writeAsStringAsync(path, data);
      }
    } catch (e) {
      console.error("Failed to persist playback", e);
    } finally {
      this.isPersisting = false;
    }
  }
}

async function chooseStore(): Promise<string> {
  let permission =
    await StorageAccessFramework.requestDirectoryPermissionsAsync(null);

  if (!permission.granted) {
    throw new Error("Permission denied");
  }

  console.log(`Got permission for ${permission.directoryUri}`);

  try {
    let info = await getInfoAsync(
      storagePath(permission.directoryUri, STATE_FILE),
    );

    if (info.exists && !info.isDirectory) {
      return permission.directoryUri;
    }

    throw new Error(`Store is not a file`);
  } catch (e) {
    throw new Error(`Failed to access store: ${e}`);
  }
}

async function applyPlaybackStates(storeLocation: string, state: State) {
  // Merge any pending playback updates written since the last Rust sync.
  try {
    let playbackStr = await StorageAccessFramework.readAsStringAsync(
      storagePath(storeLocation, PLAYBACK_FILE),
    );

    let json = JSON.parse(playbackStr);
    let result = PlaybackUpdatesDecoder.decode(json);
    if (!result.isOk()) {
      throw new Error(`Invalid state: ${result.error}`);
    }

    let updates = result.value;

    for (let [serverId, videos] of Object.entries(updates.servers ?? {})) {
      for (let [videoId, playbackState] of Object.entries(videos)) {
        let video = state.servers?.[serverId]?.videos?.[videoId];
        if (video) {
          video.playbackState = playbackState;
        }
      }
    }
    console.log("Merged pending playback updates");
  } catch {
    // Playback file missing or unreadable — not an error.
  }
}

async function loadMediaState(storeLocation: string): Promise<State> {
  console.log(`Loading media state from ${storeLocation}`);

  for (const file of [STATE_FILE, STATE_BACKUP_FILE]) {
    try {
      let stateStr = await StorageAccessFramework.readAsStringAsync(
        storagePath(storeLocation, file),
      );

      let json: State = JSON.parse(stateStr);
      let result = StateDecoder.decode(json);
      if (!result.isOk()) {
        throw new Error(`Invalid state: ${result.error}`);
      }

      let state = result.value;

      let servers = Object.values(state.servers ?? {});
      let videos = servers.flatMap((server) =>
        Object.values(server.videos ?? {}),
      );
      console.log(
        `Loaded state from ${file} with ${servers.length} servers and ${videos.length} videos.`,
      );

      await applyPlaybackStates(storeLocation, state);

      return state;
    } catch (e) {
      console.error(`State read failed from ${file}`, e);
    }
  }

  return { clientId: "", servers: {} };
}

async function findStore(): Promise<[string, State]> {
  let storeLocation: string | null = null;

  try {
    storeLocation = await AsyncStorage.getItem(STORE_KEY);

    if (storeLocation) {
      try {
        let info = await getInfoAsync(storagePath(storeLocation, STATE_FILE));

        if (!info.exists || info.isDirectory) {
          console.warn(`Previous state store is no longer a file.`);
          storeLocation = null;
        }
      } catch (e) {
        console.warn(`Failed to access previous store: ${e}`);
        storeLocation = null;
      }
    }
  } catch (e) {
    console.error(e);
  }

  while (true) {
    try {
      storeLocation ??= await chooseStore();

      await AsyncStorage.setItem(STORE_KEY, storeLocation);

      let state = await loadMediaState(storeLocation);

      return [storeLocation, state];
    } catch (e) {
      console.error(e);
      storeLocation = null;
    }
  }
}

export class DirectMediaStore extends MediaStore {
  #state: State = { clientId: "" };
  #location = "";
  #persister: StatePersister | null = null;

  get location(): string {
    return this.#location;
  }

  async init(): Promise<void> {
    let [loc, state] = await findStore();
    this.#location = loc;
    this.#state = state;
    this.#persister = new StatePersister(loc);
  }

  async pickNew(): Promise<void> {
    let loc = await chooseStore();
    await AsyncStorage.setItem(STORE_KEY, loc);
    this.#state = await loadMediaState(loc);
    this.#location = loc;
    this.#persister = new StatePersister(loc);
  }

  getServers(): Promise<Server[]> {
    return Promise.resolve(new MediaState(this.#state).servers());
  }

  getLibraries(): Promise<Library[]> {
    let servers = new MediaState(this.#state).servers();
    let libraries = servers.flatMap((s) => s.libraries());
    libraries.sort((a, b) => a.title.localeCompare(b.title));
    return Promise.resolve(libraries);
  }

  getPlaylists(): Promise<Playlist[]> {
    let servers = new MediaState(this.#state).servers();
    let playlists = servers.flatMap((s) => s.playlists());
    playlists.sort((a, b) => a.title.localeCompare(b.title));
    return Promise.resolve(playlists);
  }

  getLibrary(serverId: string, libraryId: string): Promise<Library> {
    return Promise.resolve(
      new MediaState(this.#state).getServer(serverId).getLibrary(libraryId),
    );
  }

  getCollection(serverId: string, collectionId: string): Promise<Collection> {
    return Promise.resolve(
      new MediaState(this.#state)
        .getServer(serverId)
        .getCollection(collectionId),
    );
  }

  getShow(serverId: string, showId: string): Promise<Show> {
    return Promise.resolve(
      new MediaState(this.#state).getServer(serverId).getShow(showId),
    );
  }

  getPlaylist(serverId: string, playlistId: string): Promise<Playlist> {
    return Promise.resolve(
      new MediaState(this.#state).getServer(serverId).getPlaylist(playlistId),
    );
  }

  getVideo(serverId: string, videoId: string): Promise<Video> {
    return Promise.resolve(
      new MediaState(this.#state).getServer(serverId).getVideo(videoId),
    );
  }

  resolveUri(path: string): string {
    return storagePath(this.#location, path);
  }

  async setPlaybackState(
    serverId: string,
    videoId: string,
    playbackState: PlaybackState,
  ): Promise<void> {
    let server = this.#state.servers?.[serverId];
    if (!server) return;
    let video = server.videos?.[videoId];
    if (!video) return;

    // Mutate in place so existing wrappers see the updated state
    video.playbackState = playbackState;

    if (this.#persister) {
      console.log("Persisting playback state");
      await this.#persister.persistPlayback(this.#state);
    }
  }
}
