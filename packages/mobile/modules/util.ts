import { createContext, useContext, useMemo } from "react";
import { useMediaState } from "../components/AppState";
import { LibraryState, PlaylistState } from "./state";
import {
  RouteProp,
  ParamListBase,
  NavigationProp,
} from "@react-navigation/native";

export interface ScreenProps {
  route: RouteProp<ParamListBase>;
  navigation: NavigationProp<ParamListBase>;
}

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
