import {
  FileInfo,
  StorageAccessFramework,
  getInfoAsync,
} from "expo-file-system/legacy";
import { PlaybackState, PlaybackUpdates, State } from "../state/base";
import { isDownloaded, StateDecoder } from "../state";
import { PlaybackUpdatesDecoder } from "../state/decoders";
import { Collection, Show, Video } from "../state/wrappers";
import { StateBasedMediaStore } from "./MediaStore";

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

  let errorToThrow: Error | null = null;

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
      errorToThrow ??= new Error(`State read failed from ${file}: ${e}`);
    }
  }

  throw errorToThrow!;
}

export class DirectMediaStore extends StateBasedMediaStore {
  #persister: StatePersister;

  constructor(state: State, location: string) {
    super(state, location);
    this.#persister = new StatePersister(location);
  }

  static async init(storeLocation: string): Promise<DirectMediaStore> {
    let info = await safeInfo(storagePath(storeLocation, STATE_FILE));

    if (!info?.exists || info.isDirectory) {
      throw new Error(`Store state is no longer a file.`);
    }

    let state = await loadMediaState(storeLocation);
    return new DirectMediaStore(state, storeLocation);
  }

  static async pickNewStore(): Promise<DirectMediaStore> {
    let permission =
      await StorageAccessFramework.requestDirectoryPermissionsAsync(null);

    if (!permission.granted) {
      throw new Error("Permission denied");
    }

    console.log(`Got permission for ${permission.directoryUri}`);

    return DirectMediaStore.init(permission.directoryUri);
  }

  thumbnailUri(item: Video | Show | Collection): string | undefined {
    if (item.thumbnail.state !== "stored") {
      return undefined;
    }

    return storagePath(this.location, item.thumbnail.path);
  }

  videoUri(video: Video): string | undefined {
    if (!isDownloaded(video.download)) {
      return undefined;
    }

    return storagePath(this.location, video.download.path);
  }

  async persistPlaybackState(video: Video) {
    console.log(
      `Persisting playback state for ${video.id} to ${video.playPosition}`,
    );
    await this.#persister.persistPlayback(this.state);
  }
}
