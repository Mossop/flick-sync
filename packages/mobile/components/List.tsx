import { useCallback, useEffect, useMemo, useState } from "react";
import {
  TouchableRipple,
  Text,
  Appbar,
  Menu,
  TextProps,
  Modal,
  Portal,
} from "react-native-paper";
import {
  View,
  StyleSheet,
  Image,
  LayoutChangeEvent,
  FlatList,
  StyleProp,
  ViewStyle,
} from "react-native";
import { NavigationProp, useNavigation } from "@react-navigation/native";
import { MaterialIcons } from "@expo/vector-icons";
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
  ContainerType,
} from "../state";
import {
  EPISODE_HEIGHT,
  EPISODE_WIDTH,
  PADDING,
  POSTER_HEIGHT,
  POSTER_WIDTH,
} from "../modules/styles";
import { AppRoutes, VideoParams } from "./AppNavigator";
import { byTitle, pad } from "../modules/util";
import {
  Display,
  ListSetting,
  Ordering,
  setListSettings,
  useAction,
  useListSetting,
  useStoragePath,
} from "./Store";

// Offer to start from the current position as long as it is larger than this.
const START_SLOP = 15000;

type ChildItem = Video | Collection | Playlist | Show;

const styles = StyleSheet.create({
  root: {
    paddingHorizontal: PADDING / 2,
  },
  base: {
    height: "100%",
    width: "100%",
    flex: 1,
  },

  listItem: {
    width: "100%",
    flexDirection: "row",
    alignItems: "center",
    padding: PADDING / 2,
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
    padding: PADDING / 2,
  },
  posterTitle: {
    textAlign: "center",
  },

  thumbImage: {
    height: "100%",
    width: "100%",
    resizeMode: "contain",
  },
  thumbOverlay: {
    position: "absolute",
    top: 0,
    right: 0,
    left: 0,
    bottom: 0,
    width: "100%",
    height: "100%",
    flexDirection: "column",
    justifyContent: "space-between",
    background: "red",
  },
  posterThumb: {
    width: POSTER_WIDTH,
    height: POSTER_HEIGHT,
  },
  videoThumb: {
    width: EPISODE_WIDTH,
    height: EPISODE_HEIGHT,
  },
  unplayedBadge: {
    alignSelf: "flex-end",
    paddingTop: 5,
    paddingEnd: 5,
  },
  playbackPosition: {
    height: 5,
    backgroundColor: "#e5a00d",
    alignSelf: "flex-start",
  },

  videoModal: {
    flexDirection: "row",
    alignItems: "stretch",
    justifyContent: "center",
    backgroundColor: "white",
    padding: PADDING,
    gap: PADDING * 3,
    marginLeft: "auto",
    marginRight: "auto",
  },
  modalButton: {},
  modalOption: {
    flexDirection: "column",
    alignItems: "center",
  },
});

function shouldQueue(container: ContainerType): boolean {
  switch (container) {
    case ContainerType.Playlist:
    case ContainerType.Show:
      return true;
    default:
      return false;
  }
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

      return result;
    }

    return items;
  }, [items, ordering]);
}

function OrderingMenuItem({
  title,
  ordering,
  currentOrdering,
  setOrdering,
}: {
  title: string;
  ordering: Ordering;
  currentOrdering: Ordering;
  setOrdering: (ordering: Ordering) => void;
}) {
  return (
    <Menu.Item
      leadingIcon={ordering == currentOrdering ? "check" : undefined}
      onPress={() => setOrdering(ordering)}
      title={title}
    />
  );
}

export function ListControls({
  id,
  container,
}: {
  id: string;
  container: ContainerType;
}) {
  let listSettings = useListSetting(id, container);
  let dispatchSetListSettings = useAction(setListSettings);

  let [menuVisible, setMenuVisible] = useState(false);

  let toggleDisplay = useCallback(() => {
    let newSettings: ListSetting = {
      ...listSettings,
      display:
        listSettings.display == Display.Grid ? Display.List : Display.Grid,
    };

    dispatchSetListSettings([id, newSettings]);
  }, [id, listSettings, dispatchSetListSettings]);

  let setOrdering = useCallback(
    (ordering: Ordering) => {
      let newSettings: ListSetting = {
        ...listSettings,
        ordering,
      };

      dispatchSetListSettings([id, newSettings]);
      setMenuVisible(false);
    },
    [id, listSettings, dispatchSetListSettings],
  );

  return (
    <>
      <Appbar.Action
        icon={
          listSettings.display == Display.Grid
            ? "view-grid"
            : "format-list-text"
        }
        onPress={toggleDisplay}
      />
      <Menu
        visible={menuVisible}
        onDismiss={() => setMenuVisible(false)}
        anchor={
          <Appbar.Action icon="filter" onPress={() => setMenuVisible(true)} />
        }
        anchorPosition="bottom"
      >
        {container != ContainerType.Library && (
          <OrderingMenuItem
            ordering={Ordering.Index}
            title="Order"
            currentOrdering={listSettings.ordering}
            setOrdering={setOrdering}
          />
        )}
        <OrderingMenuItem
          ordering={Ordering.Title}
          title="Title"
          currentOrdering={listSettings.ordering}
          setOrdering={setOrdering}
        />
        <OrderingMenuItem
          ordering={Ordering.AirDate}
          title="Air Date"
          currentOrdering={listSettings.ordering}
          setOrdering={setOrdering}
        />
      </Menu>
    </>
  );
}

enum ThumbnailType {
  Poster,
  // eslint-disable-next-line @typescript-eslint/no-shadow
  Video,
}

function ThumbnailOverlay({
  item,
  type,
  dimensions,
}: {
  item: Video;
  type: ThumbnailType;
  dimensions: { width: number; height: number };
}) {
  let width;
  let height;
  if (type == ThumbnailType.Poster) {
    width = POSTER_WIDTH;
    height = POSTER_HEIGHT;
  } else {
    width = EPISODE_WIDTH;
    height = EPISODE_HEIGHT;
  }

  let paddingHorizontal;
  let paddingVertical;
  if (!dimensions) {
    paddingHorizontal = 0;
    paddingVertical = 0;
  } else if (width / height > dimensions.width / dimensions.height) {
    paddingVertical = 0;
    paddingHorizontal =
      (width - (dimensions.width * height) / dimensions.height) / 2;
  } else {
    paddingHorizontal = 0;
    paddingVertical =
      (height - (dimensions.height * width) / dimensions.width) / 2;
  }

  let percentComplete = Math.floor(
    (100 * item.playPosition) / item.totalDuration,
  );

  return (
    <View style={[styles.thumbOverlay, { paddingVertical, paddingHorizontal }]}>
      <View style={styles.unplayedBadge}>
        {item.playbackState.state == "unplayed" && (
          <MaterialIcons name="stop-circle" size={16} color="#e5a00d" />
        )}
      </View>
      {item.playbackState.state == "inprogress" && (
        <View
          style={[styles.playbackPosition, { width: `${percentComplete}%` }]}
        />
      )}
    </View>
  );
}

function Thumbnail({ item, type }: { item: ChildItem; type: ThumbnailType }) {
  let storagePath = useStoragePath();
  let [dimensions, setDimensions] = useState<{
    width: number;
    height: number;
  }>();

  let uri =
    !(item instanceof Playlist) && item.thumbnail.state == "downloaded"
      ? storagePath(item.thumbnail.path)
      : undefined;

  useEffect(() => {
    if (uri) {
      Image.getSize(uri, (width, height) => {
        setDimensions({ width, height });
      });
    }
  }, [uri]);

  let style =
    type == ThumbnailType.Poster ? styles.posterThumb : styles.videoThumb;

  if (!uri) {
    return <View style={style} />;
  }

  return (
    <View style={style}>
      <Image source={{ uri }} style={[styles.thumbImage, style]} />
      {dimensions && isVideo(item) && (
        <ThumbnailOverlay item={item} dimensions={dimensions} type={type} />
      )}
    </View>
  );
}

function MetaTitle(props: TextProps<never>) {
  return <Text variant="titleMedium" {...props} />;
}

function MetaSub(props: TextProps<never>) {
  return (
    <Text
      variant="labelMedium"
      numberOfLines={1}
      ellipsizeMode="tail"
      {...props}
    />
  );
}

function MetaInfo(props: TextProps<never>) {
  return (
    <Text
      variant="labelSmall"
      numberOfLines={1}
      ellipsizeMode="tail"
      {...props}
    />
  );
}

function ListMeta({ item }: { item: ChildItem }) {
  if (item instanceof Episode) {
    return (
      <View style={styles.listMeta}>
        <MetaTitle>{item.title}</MetaTitle>
        <MetaSub>
          s{pad(item.season.index)}e{pad(item.index)} - {item.season.show.title}
        </MetaSub>
        <MetaInfo>{duration(item)}</MetaInfo>
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
        <MetaTitle>{item.title}</MetaTitle>
        <MetaSub>
          {seasons.length} seasons, {episodes} episodes
        </MetaSub>
        <MetaInfo>{duration(item)}</MetaInfo>
      </View>
    );
  }

  if (item instanceof Season) {
    return (
      <View style={styles.listMeta}>
        <MetaTitle>{item.title}</MetaTitle>
        <MetaSub>{item.episodes.length} episodes</MetaSub>
        <MetaInfo>{duration(item)}</MetaInfo>
      </View>
    );
  }

  if (item instanceof ShowCollection) {
    return (
      <View style={styles.listMeta}>
        <MetaTitle>{item.title}</MetaTitle>
        <MetaSub>{item.contents.length} shows</MetaSub>
        <MetaInfo>{duration(item)}</MetaInfo>
      </View>
    );
  }

  if (item instanceof MovieCollection) {
    return (
      <View style={styles.listMeta}>
        <MetaTitle>{item.title}</MetaTitle>
        <MetaSub>{item.contents.length} movies</MetaSub>
        <MetaInfo>{duration(item)}</MetaInfo>
      </View>
    );
  }

  if (item instanceof Playlist) {
    return (
      <View style={styles.listMeta}>
        <MetaTitle>{item.title}</MetaTitle>
        <MetaSub>{item.videos.length} videos</MetaSub>
        <MetaInfo>{duration(item)}</MetaInfo>
      </View>
    );
  }

  return (
    <View style={styles.listMeta}>
      <MetaTitle>{item.title}</MetaTitle>
      <MetaInfo>{duration(item)}</MetaInfo>
    </View>
  );
}

function GridItem<T extends ChildItem>({
  item,
  index,
  width,
  onClick,
}: {
  item: T;
  index: number;
  width: number;
  onClick: (item: T, index: number) => void;
}) {
  return (
    <TouchableRipple onPress={() => onClick(item, index)}>
      <View style={[styles.poster, { width }]}>
        <Thumbnail type={ThumbnailType.Poster} item={item} />
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
  );
}

function ListItem<T extends ChildItem>({
  item,
  index,
  onClick,
}: {
  item: T;
  index: number;
  onClick: (item: T, index: number) => void;
}) {
  return (
    <TouchableRipple key={item.id} onPress={() => onClick(item, index)}>
      <View style={styles.listItem}>
        <Thumbnail
          type={
            item instanceof Episode ? ThumbnailType.Video : ThumbnailType.Poster
          }
          item={item}
        />
        <ListMeta item={item} />
      </View>
    </TouchableRipple>
  );
}

function VideoStartModal({
  video,
  container,
  queue,
  index,
  onDismiss,
}: {
  video: Movie | Episode;
  container: ContainerType;
  queue: string[];
  index: number;
  onDismiss: () => void;
}) {
  let navigation = useNavigation<NavigationProp<AppRoutes>>();

  let params: VideoParams = useMemo(() => {
    let willQueue = shouldQueue(container);
    return {
      server: video.library.server.id,
      queue: willQueue ? queue : [video.id],
      index: willQueue ? index : 0,
    };
  }, [video, queue, index, container]);

  let onPlay = useCallback(() => {
    navigation.navigate("video", params);
    onDismiss();
  }, [params, navigation, onDismiss]);

  let onRestart = useCallback(() => {
    navigation.navigate("video", {
      ...params,
      restart: true,
    });
    onDismiss();
  }, [params, navigation, onDismiss]);

  return (
    <Portal>
      <Modal
        visible
        onDismiss={onDismiss}
        contentContainerStyle={styles.videoModal}
      >
        <TouchableRipple onPress={onPlay} style={styles.modalButton}>
          <View style={styles.modalOption}>
            <MaterialIcons name="play-arrow" size={80} />
            <Text variant="labelMedium" numberOfLines={1}>
              Play
            </Text>
          </View>
        </TouchableRipple>
        <TouchableRipple onPress={onRestart} style={styles.modalButton}>
          <View style={styles.modalOption}>
            <MaterialIcons name="replay" size={80} />
            <Text variant="labelMedium" numberOfLines={1}>
              Restart
            </Text>
          </View>
        </TouchableRipple>
      </Modal>
    </Portal>
  );
}

export function List<T extends ChildItem>({
  id,
  container,
  items,
  style,
  onClick,
}: {
  id: string;
  container: ContainerType;
  style?: StyleProp<ViewStyle>;
  items: readonly T[];
  onClick?: (item: T) => void;
}) {
  let listSettings = useListSetting(id, container);
  let sorted = useSorted(items, listSettings.ordering);
  let navigation = useNavigation<NavigationProp<AppRoutes>>();
  let [dimensions, setDimensions] = useState<{
    width: number;
    height: number;
  }>();
  let [startingVideo, setStartingVideo] = useState<[Movie | Episode, number]>();
  let queue = useMemo(() => sorted.map((i) => i.id), [sorted]);

  let itemClick = useCallback(
    (item: T, index: number) => {
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
        if (item.playPosition > START_SLOP) {
          setStartingVideo([item, index]);
        } else {
          let willQueue = shouldQueue(container);

          navigation.navigate("video", {
            server: item.library.server.id,
            queue: willQueue ? queue : [item.id],
            index: willQueue ? index : 0,
          });
        }
      }

      if (item instanceof ShowCollection || item instanceof MovieCollection) {
        navigation.navigate("collection", {
          server: item.library.server.id,
          collection: item.id,
        });
      }
    },
    [onClick, queue, container, navigation],
  );

  let updateDimensions = useCallback(
    (event: LayoutChangeEvent) => {
      if (
        event.nativeEvent.layout.width != dimensions?.width ||
        event.nativeEvent.layout.height != dimensions?.height
      ) {
        setDimensions(event.nativeEvent.layout);
      }
    },
    [dimensions],
  );

  let [numColumns, width, initialNumToRender] = useMemo(() => {
    if (!dimensions) {
      return [0, 0, 0];
    }

    let availWidth = dimensions.width - PADDING;
    let rows = Math.ceil(dimensions.height / POSTER_HEIGHT);
    let columns = 1;

    if (listSettings.display == Display.Grid) {
      columns = Math.floor(availWidth / (POSTER_WIDTH + PADDING));
    }

    return [columns, availWidth / columns, columns * rows];
  }, [listSettings.display, dimensions]);

  let renderItem = useCallback(
    // eslint-disable-next-line react/no-unused-prop-types
    ({ item, index }: { item: T; index: number }) => {
      if (listSettings.display == Display.Grid) {
        return (
          <GridItem
            item={item}
            index={index}
            width={width}
            onClick={itemClick}
          />
        );
      }
      return <ListItem item={item} index={index} onClick={itemClick} />;
    },
    [itemClick, width, listSettings.display],
  );

  return (
    <View
      onLayout={updateDimensions}
      style={[styles.root, style ?? styles.base]}
    >
      {startingVideo && (
        <VideoStartModal
          container={container}
          video={startingVideo[0]}
          queue={queue}
          index={startingVideo[1]}
          onDismiss={() => setStartingVideo(undefined)}
        />
      )}
      {dimensions && (
        <FlatList
          key={`${listSettings.display}${numColumns}`}
          data={sorted}
          numColumns={numColumns}
          initialNumToRender={initialNumToRender}
          keyExtractor={(item) => item.id}
          renderItem={renderItem}
        />
      )}
    </View>
  );
}
