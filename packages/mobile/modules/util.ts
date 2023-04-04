import { useSearchParams } from "expo-router";
import { useMemo } from "react";
import { useMediaState } from "../components/AppState";
import { LibraryState, PlaylistState } from "./state";

export function useLibraries(): LibraryState[] {
  let mediaState = useMediaState();

  return useMemo(() => {
    let libraries = Array.from(mediaState.servers.values()).flatMap((server) =>
      Array.from(server.libraries.values())
    );

    libraries.sort((a, b) => a.title.localeCompare(b.title));

    return libraries;
  }, [mediaState]);
}

export function usePlaylists(): PlaylistState[] {
  let mediaState = useMediaState();

  return useMemo(() => {
    let playlists = Array.from(mediaState.servers.values()).flatMap((server) =>
      Array.from(server.playlists.values())
    );

    playlists.sort((a, b) => a.title.localeCompare(b.title));

    return playlists;
  }, [mediaState]);
}

export function useLibrary(): LibraryState {
  let params = useSearchParams();
  let libraries = useLibraries();

  return (
    libraries.find(
      (lib) =>
        lib.id.toString() == params.library && lib.server.id == params.server
    ) ?? libraries[0]
  );
}

export function usePlaylist(): PlaylistState | undefined {
  let params = useSearchParams();
  let playlists = usePlaylists();

  return playlists.find(
    (pl) => pl.id.toString() == params.playlist && pl.server.id == params.server
  );
}
