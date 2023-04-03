import { useSearchParams } from "expo-router";
import { useMemo } from "react";
import { useMediaState } from "../components/AppState";
import { LibraryState, PlaylistState } from "./ruststate";

export type Library = LibraryState & {
  server: string;
};

export function useLibraries(): Library[] {
  let mediaState = useMediaState();

  return useMemo(() => {
    let libraries: Library[] = [];

    for (let [id, server] of Object.entries(mediaState.servers)) {
      for (let library of server.libraries) {
        libraries.push({
          server: id,
          ...library,
        });
      }
    }

    libraries.sort((a, b) => a.title.localeCompare(b.title));

    return libraries;
  }, [mediaState]);
}

export type Playlist = PlaylistState & {
  server: string;
};

export function usePlaylists(): Playlist[] {
  let mediaState = useMediaState();

  return useMemo(() => {
    let playlists: Playlist[] = [];

    for (let [id, server] of Object.entries(mediaState.servers)) {
      for (let playlist of server.playlists) {
        playlists.push({
          server: id,
          ...playlist,
        });
      }
    }

    playlists.sort((a, b) => a.title.localeCompare(b.title));

    return playlists;
  }, [mediaState]);
}

export function useLibrary(): Library {
  let params = useSearchParams();
  let libraries = useLibraries();

  return (
    libraries.find(
      (lib) =>
        lib.id.toString() == params.library && lib.server == params.server
    ) ?? libraries[0]
  );
}

export function usePlaylist(): Playlist {
  let params = useSearchParams();
  let playlists = usePlaylists();

  return (
    playlists.find(
      (pl) => pl.id.toString() == params.playlist && pl.server == params.server
    ) ?? playlists[0]
  );
}
