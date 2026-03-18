import { use, useMemo } from "react";
import { useMediaStore } from "./MediaStoreProvider";
import {
  Library,
  Collection,
  Show,
  Playlist,
  Video,
  Server,
} from "../state/wrappers";

export function useServers(): Server[] {
  let store = useMediaStore();
  let promise = useMemo(() => store.getServers(), [store]);
  return use(promise);
}

export function useLibraries(): Library[] {
  let store = useMediaStore();
  let promise = useMemo(() => store.getLibraries(), [store]);
  return use(promise);
}

export function usePlaylists(): Playlist[] {
  let store = useMediaStore();
  let promise = useMemo(() => store.getPlaylists(), [store]);
  return use(promise);
}

export function useLibrary(serverId: string, libraryId: string): Library {
  let store = useMediaStore();
  let promise = useMemo(
    () => store.getLibrary(serverId, libraryId),
    [store, serverId, libraryId],
  );
  return use(promise);
}

export function useCollection(
  serverId: string,
  collectionId: string,
): Collection {
  let store = useMediaStore();
  let promise = useMemo(
    () => store.getCollection(serverId, collectionId),
    [store, serverId, collectionId],
  );
  return use(promise);
}

export function useShow(serverId: string, showId: string): Show {
  let store = useMediaStore();
  let promise = useMemo(
    () => store.getShow(serverId, showId),
    [store, serverId, showId],
  );
  return use(promise);
}

export function usePlaylist(serverId: string, playlistId: string): Playlist {
  let store = useMediaStore();
  let promise = useMemo(
    () => store.getPlaylist(serverId, playlistId),
    [store, serverId, playlistId],
  );
  return use(promise);
}

export function useVideo(serverId: string, videoId: string): Video {
  let store = useMediaStore();
  let promise = useMemo(
    () => store.getVideo(serverId, videoId),
    [store, serverId, videoId],
  );
  return use(promise);
}

export function useResolveUri(): (path: string) => string {
  let store = useMediaStore();
  return useMemo(() => (path: string) => store.resolveUri(path), [store]);
}
