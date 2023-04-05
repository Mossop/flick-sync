import { useMemo } from "react";
import { RouteProp, NavigationProp } from "@react-navigation/native";
import { useMediaState } from "../components/AppState";
import { LibraryState, PlaylistState } from "./state";

interface LibraryParams {
  server: string;
  library: number;
}

interface PlaylistParams {
  server: string;
  playlist: number;
}

export interface Routes {
  library: LibraryParams | undefined;
  playlist: PlaylistParams;
  [key: string]: object | undefined;
}

export interface ScreenProps<Screen extends keyof Routes = keyof Routes> {
  route: RouteProp<Routes, Screen>;
  navigation: NavigationProp<Routes, Screen>;
}

export function useLibraries(): LibraryState[] {
  let mediaState = useMediaState();

  return useMemo(() => {
    let libraries = Array.from(mediaState.servers.values()).flatMap((server) =>
      Array.from(server.libraries.values()),
    );

    libraries.sort((a, b) => a.title.localeCompare(b.title));

    return libraries;
  }, [mediaState]);
}

export function usePlaylists(): PlaylistState[] {
  let mediaState = useMediaState();

  return useMemo(() => {
    let playlists = Array.from(mediaState.servers.values()).flatMap((server) =>
      Array.from(server.playlists.values()),
    );

    playlists.sort((a, b) => a.title.localeCompare(b.title));

    return playlists;
  }, [mediaState]);
}
