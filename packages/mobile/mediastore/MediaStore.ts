import { PlaybackState, State } from "../state/base";
import {
  Server,
  Library,
  Collection,
  Show,
  Playlist,
  Video,
  MediaState,
} from "../state/wrappers";

export abstract class MediaStore {
  protected constructor(public readonly location: string) {}

  // Navigation / data access (granular, async)
  abstract getServers(): Promise<Server[]>;
  abstract getLibrary(serverId: string, libraryId: string): Promise<Library>;
  abstract getCollection(
    serverId: string,
    collectionId: string,
  ): Promise<Collection>;
  abstract getShow(serverId: string, showId: string): Promise<Show>;
  abstract getPlaylist(serverId: string, playlistId: string): Promise<Playlist>;
  abstract getVideo(serverId: string, videoId: string): Promise<Video>;

  // Convenience: all libraries/playlists across servers (for Drawer)
  abstract getLibraries(): Promise<Library[]>;
  abstract getPlaylists(): Promise<Playlist[]>;

  abstract thumbnailUri(item: Video | Show | Collection): string | undefined;
  abstract videoUri(video: Video): string | undefined;

  // Playback persistence
  abstract setPlaybackState(
    video: Video,
    playbackState: PlaybackState,
  ): Promise<void>;

  static async loadStore(storeLocation: string): Promise<MediaStore | null> {
    if (storeLocation.startsWith("content:")) {
      const { DirectMediaStore } = await import("./DirectMediaStore");

      return DirectMediaStore.init(storeLocation);
    }

    if (storeLocation.startsWith("http:")) {
      const { UpnpMediaStore } = await import("./UpnpMediaStore");

      return UpnpMediaStore.init(storeLocation);
    }

    throw new Error(`Unknown store location ${storeLocation}`);
  }
}

export abstract class StateBasedMediaStore extends MediaStore {
  #mediaState: MediaState;

  constructor(
    protected readonly state: State,
    location: string,
  ) {
    super(location);
    this.#mediaState = new MediaState(state);
  }

  getServers(): Promise<Server[]> {
    return Promise.resolve(this.#mediaState.servers());
  }

  getLibraries(): Promise<Library[]> {
    let servers = this.#mediaState.servers();
    let libraries = servers.flatMap((s) => s.libraries());
    libraries.sort((a, b) => a.title.localeCompare(b.title));
    return Promise.resolve(libraries);
  }

  getPlaylists(): Promise<Playlist[]> {
    let servers = this.#mediaState.servers();
    let playlists = servers.flatMap((s) => s.playlists());
    playlists.sort((a, b) => a.title.localeCompare(b.title));
    return Promise.resolve(playlists);
  }

  getLibrary(serverId: string, libraryId: string): Promise<Library> {
    return Promise.resolve(
      this.#mediaState.getServer(serverId).getLibrary(libraryId),
    );
  }

  getCollection(serverId: string, collectionId: string): Promise<Collection> {
    return Promise.resolve(
      this.#mediaState.getServer(serverId).getCollection(collectionId),
    );
  }

  getShow(serverId: string, showId: string): Promise<Show> {
    return Promise.resolve(
      this.#mediaState.getServer(serverId).getShow(showId),
    );
  }

  getPlaylist(serverId: string, playlistId: string): Promise<Playlist> {
    return Promise.resolve(
      this.#mediaState.getServer(serverId).getPlaylist(playlistId),
    );
  }

  getVideo(serverId: string, videoId: string): Promise<Video> {
    return Promise.resolve(
      this.#mediaState.getServer(serverId).getVideo(videoId),
    );
  }

  setPlaybackState(video: Video, playbackState: PlaybackState): Promise<void> {
    let serverState = this.state.servers?.[video.library.server.id];
    if (!serverState) {
      return Promise.resolve();
    }

    let videoState = serverState.videos?.[video.id];
    if (!videoState) {
      return Promise.resolve();
    }

    // Mutate in place so existing wrappers see the updated state
    videoState.playbackState = playbackState;

    return Promise.resolve();
  }
}
