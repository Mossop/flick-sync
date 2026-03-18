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
  // Initialization (called once at startup)
  abstract init(): Promise<void>;

  // Store identity (for settings keying & display)
  abstract get location(): string;

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
    serverId: string,
    videoId: string,
    state: PlaybackState,
  ): Promise<void>;

  // Store switching
  abstract pickNew(): Promise<void>;
}
