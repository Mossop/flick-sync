import { PlaybackState } from "../state/base";
import {
  Server,
  Library,
  Collection,
  Show,
  Playlist,
  Video,
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

  // URI resolution (synchronous - constructs URI from base + relative path)
  abstract resolveUri(path: string): string;

  // Playback persistence
  abstract setPlaybackState(
    video: Video,
    playbackState: PlaybackState,
  ): Promise<void>;

  static async loadStore(storeLocation: string): Promise<MediaStore | null> {
    const { DirectMediaStore } = await import("./DirectMediaStore");

    return DirectMediaStore.init(storeLocation);
  }
}
