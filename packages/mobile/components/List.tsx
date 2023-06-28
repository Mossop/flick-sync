import { useCallback, useMemo } from "react";
import { TouchableRipple, Text, Appbar } from "react-native-paper";
import { View, StyleSheet } from "react-native";
import { NavigationProp, useNavigation } from "@react-navigation/native";
import {
  Collection,
  Video,
  Playlist,
  Episode,
  Movie,
  Show,
  Season,
  ShowCollection,
  MovieCollection,
  isVideo,
} from "../state";
import { useSettings } from "./AppState";
import GridView from "./GridView";
import {
  EPISODE_HEIGHT,
  EPISODE_WIDTH,
  PADDING,
  POSTER_HEIGHT,
  POSTER_WIDTH,
} from "../modules/styles";
import Thumbnail from "./Thumbnail";
import { AppRoutes } from "./AppNavigator";
import { byTitle } from "../modules/util";

export enum Type {
  // eslint-disable-next-line @typescript-eslint/no-shadow
  Collection,
  // eslint-disable-next-line @typescript-eslint/no-shadow
  Playlist,
  // eslint-disable-next-line @typescript-eslint/no-shadow
  Movie,
  // eslint-disable-next-line @typescript-eslint/no-shadow
  Episode,
  // eslint-disable-next-line @typescript-eslint/no-shadow
  Show,
  PlaylistItem,
}

export enum Display {
  Grid = "grid",
  // eslint-disable-next-line @typescript-eslint/no-shadow
  List = "list",
}

export enum Ordering {
  Index = "index",
  Title = "title",
  AirDate = "airdate",
}

type ChildItem = Video | Collection | Playlist | Show;

export interface ListSetting {
  display: Display;
  ordering: Ordering;
}

const styles = StyleSheet.create({
  list: {
    alignItems: "stretch",
    padding: PADDING,
  },
  listItem: {
    flexDirection: "row",
    alignItems: "center",
    paddingBottom: PADDING,
  },
  listThumbContainer: {
    width: Math.max(EPISODE_WIDTH, POSTER_WIDTH),
    alignItems: "center",
    justifyContent: "center",
  },
  episodeThumb: {
    width: EPISODE_WIDTH,
    height: EPISODE_HEIGHT,
  },
  listMeta: {
    flex: 1,
    paddingLeft: PADDING,
    flexDirection: "column",
    alignItems: "flex-start",
    justifyContent: "center",
  },

  poster: {
    flexDirection: "column",
    alignItems: "center",
    gap: PADDING,
  },
  posterTitle: {
    textAlign: "center",
  },
  posterThumb: {
    width: POSTER_WIDTH,
    height: POSTER_HEIGHT,
  },
});

function pad(val: number) {
  return val >= 10 ? `${val}` : `0${val}`;
}

function itemDuration(item: ChildItem | Season): number {
  if (item instanceof Episode) {
    return item.totalDuration;
  }
  if (item instanceof Movie) {
    return item.totalDuration;
  }
  if (item instanceof Show) {
    return item.seasons.reduce(
      (total, season) => total + itemDuration(season),
      0,
    );
  }
  if (item instanceof Season) {
    return item.episodes.reduce(
      (total, episode) => total + itemDuration(episode),
      0,
    );
  }
  if (item instanceof ShowCollection) {
    return item.contents.reduce((total, show) => total + itemDuration(show), 0);
  }
  if (item instanceof MovieCollection) {
    return item.contents.reduce(
      (total, movie) => total + itemDuration(movie),
      0,
    );
  }
  if (item instanceof Playlist) {
    return item.videos.reduce((total, video) => total + itemDuration(video), 0);
  }

  return 0;
}

function duration(item: ChildItem) {
  let secs = Math.floor(itemDuration(item) / 1000);

  let result = `${pad(secs % 60)}`;
  if (secs > 60) {
    let mins = Math.floor(secs / 60);
    result = `${pad(mins % 60)}:${result}`;

    if (mins > 60) {
      let hours = Math.floor(mins / 60);
      result = `${hours}:${result}`;
    }
  }

  return result;
}

function defaultSetting(type: Type): ListSetting {
  switch (type) {
    case Type.Episode:
      return {
        display: Display.List,
        ordering: Ordering.AirDate,
      };
    case Type.PlaylistItem:
      return {
        display: Display.List,
        ordering: Ordering.Index,
      };
    default:
      return {
        display: Display.Grid,
        ordering: Ordering.Title,
      };
  }
}

function useListSetting(id: string, type: Type) {
  let settings = useSettings();
  return settings.getListSetting(id) ?? defaultSetting(type);
}

function useSorted<T extends ChildItem>(
  items: readonly T[],
  ordering: Ordering,
) {
  return useMemo(() => {
    if (ordering == Ordering.Index) {
      return items;
    }

    if (ordering == Ordering.Title) {
      return byTitle(items);
    }

    if (ordering == Ordering.AirDate) {
      let result = [...items];
      result.sort((a, b) => {
        if (isVideo(a) && isVideo(b)) {
          return a.airDate.localeCompare(b.airDate);
        }

        return a.title.localeCompare(b.title);
      });
    }

    return items;
  }, [items, ordering]);
}

export function ListControls({ id, type }: { id: string; type: Type }) {
  let settings = useSettings();
  let listSettings = useListSetting(id, type);

  let toggleDisplay = useCallback(() => {
    let newSettings: ListSetting = {
      ...listSettings,
      display:
        listSettings.display == Display.Grid ? Display.List : Display.Grid,
    };

    settings.setListSetting(id, newSettings);
  }, [id, listSettings, settings]);

  return (
    <Appbar.Action
      icon={
        listSettings.display == Display.Grid ? "view-grid" : "format-list-text"
      }
      onPress={toggleDisplay}
    />
  );
}

function ListThumb({ item }: { item: ChildItem }) {
  let thumbStyles =
    item instanceof Episode ? styles.episodeThumb : styles.posterThumb;

  if (item instanceof Playlist || item instanceof Season) {
    return <View style={thumbStyles} />;
  }
  return <Thumbnail style={thumbStyles} thumbnail={item.thumbnail} />;
}

function PosterThumb({ item }: { item: ChildItem }) {
  if (item instanceof Playlist || item instanceof Season) {
    return <View style={styles.posterThumb} />;
  }
  return <Thumbnail style={styles.posterThumb} thumbnail={item.thumbnail} />;
}

function ListMeta({ item }: { item: ChildItem }) {
  if (item instanceof Episode) {
    return (
      <View style={styles.listMeta}>
        <Text variant="titleMedium">{item.title}</Text>
        <Text variant="labelMedium" numberOfLines={1} ellipsizeMode="tail">
          s{pad(item.season.index)}e{pad(item.index)} - {item.season.show.title}
        </Text>
        <Text variant="labelSmall">{duration(item)}</Text>
      </View>
    );
  }

  if (item instanceof Show) {
    let { seasons } = item;
    let episodes = seasons.reduce(
      (total, season) => total + season.episodes.length,
      0,
    );

    return (
      <View style={styles.listMeta}>
        <Text variant="titleMedium">{item.title}</Text>
        <Text variant="labelMedium" numberOfLines={1} ellipsizeMode="tail">
          {seasons.length} seasons, {episodes} episodes
        </Text>
        <Text variant="labelSmall">{duration(item)}</Text>
      </View>
    );
  }

  if (item instanceof Season) {
    return (
      <View style={styles.listMeta}>
        <Text variant="titleMedium">{item.title}</Text>
        <Text variant="labelMedium" numberOfLines={1} ellipsizeMode="tail">
          {item.episodes.length} episodes
        </Text>
        <Text variant="labelSmall">{duration(item)}</Text>
      </View>
    );
  }

  if (item instanceof ShowCollection) {
    return (
      <View style={styles.listMeta}>
        <Text variant="titleMedium">{item.title}</Text>
        <Text variant="labelMedium" numberOfLines={1} ellipsizeMode="tail">
          {item.contents.length} shows
        </Text>
        <Text variant="labelSmall">{duration(item)}</Text>
      </View>
    );
  }

  if (item instanceof MovieCollection) {
    return (
      <View style={styles.listMeta}>
        <Text variant="titleMedium">{item.title}</Text>
        <Text variant="labelMedium" numberOfLines={1} ellipsizeMode="tail">
          {item.contents.length} movies
        </Text>
        <Text variant="labelSmall">{duration(item)}</Text>
      </View>
    );
  }

  if (item instanceof Playlist) {
    return (
      <View style={styles.listMeta}>
        <Text variant="titleMedium">{item.title}</Text>
        <Text variant="labelMedium" numberOfLines={1} ellipsizeMode="tail">
          {item.videos.length} videos
        </Text>
        <Text variant="labelSmall">{duration(item)}</Text>
      </View>
    );
  }

  return (
    <View style={styles.listMeta}>
      <Text variant="titleMedium">{item.title}</Text>
      <Text variant="labelSmall">{duration(item)}</Text>
    </View>
  );
}

export function List<T extends ChildItem>({
  id,
  type,
  items,
  onClick,
}: {
  id: string;
  type: Type;
  items: readonly T[];
  onClick?: (item: T) => void;
}) {
  let listSettings = useListSetting(id, type);
  let sorted = useSorted(items, listSettings.ordering);
  let navigation = useNavigation<NavigationProp<AppRoutes>>();

  let itemClick = useCallback(
    (item: T) => {
      if (onClick) {
        onClick(item);
        return;
      }

      if (item instanceof Playlist) {
        navigation.navigate("playlist", {
          server: item.server.id,
          playlist: item.id,
        });
      }

      if (item instanceof Show) {
        navigation.navigate("show", {
          server: item.library.server.id,
          show: item.id,
        });
      }

      if (item instanceof Movie || item instanceof Episode) {
        navigation.navigate("video", {
          server: item.library.server.id,
          video: item.id,
        });
      }

      if (item instanceof ShowCollection || item instanceof MovieCollection) {
        navigation.navigate("collection", {
          server: item.library.server.id,
          collection: item.id,
        });
      }
    },
    [onClick, navigation],
  );

  if (listSettings.display == Display.Grid) {
    return (
      <GridView itemWidth={POSTER_WIDTH}>
        {sorted.map((item) => (
          <GridView.Item key={item.id}>
            <TouchableRipple onPress={() => itemClick(item)}>
              <View style={styles.poster}>
                <PosterThumb item={item} />
                <Text
                  variant="labelSmall"
                  style={styles.posterTitle}
                  numberOfLines={1}
                  ellipsizeMode="tail"
                >
                  {item.title}
                </Text>
              </View>
            </TouchableRipple>
          </GridView.Item>
        ))}
      </GridView>
    );
  }

  return (
    <View style={styles.list}>
      {sorted.map((item) => (
        <TouchableRipple key={item.id} onPress={() => itemClick(item)}>
          <View style={styles.listItem}>
            <View style={styles.listThumbContainer}>
              <ListThumb item={item} />
            </View>
            <ListMeta item={item} />
          </View>
        </TouchableRipple>
      ))}
    </View>
  );
}
