import { useMemo } from "react";
import {
  RouteProp,
  NavigationProp,
  ParamListBase,
} from "@react-navigation/native";
import { useMediaState } from "../components/AppState";
import { LibraryState, MovieState, PlaylistState, ShowState } from "./state";

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

function sorted<T>(list: T[], comparator: (a: T, b: T) => number): T[] {
  let result = [...list];

  result.sort(comparator);
  return result;
}

export function moviesByYear(movies: MovieState[]): MovieState[] {
  return sorted(movies, (a, b) => a.detail.year - b.detail.year);
}

export function showsByYear(movies: ShowState[]): ShowState[] {
  return sorted(movies, (a, b) => a.year - b.year);
}

function plain(st: string): string {
  return st;
}

export function byTitle<T extends { title: string }>(items: T[]): T[] {
  return sorted(items, (a, b) => plain(a.title).localeCompare(plain(b.title)));
}

export function useMapped<T>(val: T[], mapper: (val: T[]) => T[]): T[] {
  return useMemo(() => mapper(val), [val]);
}
