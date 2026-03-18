import AsyncStorage from "@react-native-async-storage/async-storage";
import { PlaybackState } from "../state/base";
import {
  Server,
  Library,
  Collection,
  Show,
  Playlist,
  Video,
} from "../state/wrappers";

const STORE_KEY = "store";

export abstract class MediaStore {
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

  static async setCurrentStore(store: MediaStore) {
    await AsyncStorage.setItem(STORE_KEY, store.location);
  }

  static async loadCurrentStore(): Promise<MediaStore | null> {
    let storeLocation: string | null = null;

    storeLocation = await AsyncStorage.getItem(STORE_KEY);

    if (!storeLocation) {
      return null;
    }

    const { DirectMediaStore } = await import("./DirectMediaStore");

    return DirectMediaStore.init(storeLocation);
  }
}
