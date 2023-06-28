import { useMemo } from "react";
import {
  RouteProp,
  NavigationProp,
  ParamListBase,
} from "@react-navigation/native";
import { MaterialIcons } from "@expo/vector-icons";
import { TextProps } from "react-native";
import { useMediaState } from "../components/AppState";
import { Episode, Library, Movie, Playlist, Show } from "../state";

export interface ScreenProps<
  Params extends ParamListBase = ParamListBase,
  Screen extends keyof Params = keyof Params,
> {
  route: RouteProp<Params, Screen>;
  navigation: NavigationProp<Params, Screen>;
}

export function useLibraries(): Library[] {
  let mediaState = useMediaState();

  return useMemo(() => {
    let libraries = mediaState
      .servers()
      .flatMap((server) => server.libraries());

    libraries.sort((a, b) => a.title.localeCompare(b.title));

    return libraries;
  }, [mediaState]);
}

export function usePlaylists(): Playlist[] {
  let mediaState = useMediaState();

  return useMemo(() => {
    let playlists = mediaState
      .servers()
      .flatMap((server) => server.playlists());

    playlists.sort((a, b) => a.title.localeCompare(b.title));

    return playlists;
  }, [mediaState]);
}

function sorted<T>(
  list: readonly T[],
  comparator: (a: T, b: T) => number,
): T[] {
  let result = [...list];

  result.sort(comparator);
  return result;
}

export function byIndex(episodes: readonly Episode[]): Episode[] {
  return sorted(episodes, (a, b) => {
    if (a.season.index == b.season.index) {
      return a.index - b.index;
    }
    return a.season.index - b.season.index;
  });
}

export function moviesByYear(movies: readonly Movie[]): Movie[] {
  return sorted(movies, (a, b) => a.year - b.year);
}

export function showsByYear(movies: readonly Show[]): Show[] {
  return sorted(movies, (a, b) => a.year - b.year);
}

function plain(st: string): string {
  let lower = st.toLocaleLowerCase().trim();
  if (lower.startsWith("the ")) {
    return lower.substring(4);
  }
  if (lower.startsWith("a ")) {
    return lower.substring(2);
  }

  return lower;
}

export function byTitle<T extends { title: string }>(items: readonly T[]): T[] {
  return sorted(items, (a, b) => plain(a.title).localeCompare(plain(b.title)));
}

export function useMapped<T>(
  val: readonly T[],
  mapper: (val: readonly T[]) => T[],
): T[] {
  return useMemo(() => mapper(val), [mapper, val]);
}

export function namedIcon(icon: string) {
  return function Icon(props: TextProps) {
    // @ts-ignore
    return <MaterialIcons name={icon} {...props} />;
  };
}
