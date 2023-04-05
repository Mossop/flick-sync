import { useMemo } from "react";
import {
  RouteProp,
  NavigationProp,
  ParamListBase,
} from "@react-navigation/native";
import { useMediaState } from "../components/AppState";
import { LibraryState, PlaylistState } from "./state";

export interface ScreenProps<
  Params extends ParamListBase = ParamListBase,
  Screen extends keyof Params = keyof Params,
> {
  route: RouteProp<Params, Screen>;
  navigation: NavigationProp<Params, Screen>;
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
