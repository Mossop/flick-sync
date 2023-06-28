import { StyleSheet, View } from "react-native";
import { TouchableRipple, Text } from "react-native-paper";
import { NavigationProp, useNavigation } from "@react-navigation/native";
import { Episode, Movie, Video, isMovie } from "../state";
import Thumbnail from "./Thumbnail";
import {
  EPISODE_WIDTH,
  EPISODE_HEIGHT,
  PADDING,
  POSTER_HEIGHT,
  POSTER_WIDTH,
} from "../modules/styles";
import { AppRoutes } from "./AppNavigator";

const styles = StyleSheet.create({
  video: {
    flexDirection: "row",
    alignItems: "center",
    paddingBottom: PADDING,
  },
  meta: {
    flex: 1,
    paddingLeft: PADDING,
    flexDirection: "column",
    alignItems: "flex-start",
    justifyContent: "center",
  },
  thumbContainer: {
    width: Math.max(EPISODE_WIDTH, POSTER_WIDTH),
    alignItems: "center",
    justifyContent: "center",
  },
  posterThumb: {
    width: POSTER_WIDTH,
    height: POSTER_HEIGHT,
  },
  episodeThumb: {
    width: EPISODE_WIDTH,
    height: EPISODE_HEIGHT,
  },
});

function pad(val: number) {
  return val >= 10 ? `${val}` : `0${val}`;
}

function duration(val: number) {
  let secs = Math.floor(val / 1000);

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

function EpisodeMeta({ episode }: { episode: Episode }) {
  return (
    <View style={styles.meta}>
      <Text variant="titleMedium">{episode.title}</Text>
      <Text variant="labelMedium" numberOfLines={1} ellipsizeMode="tail">
        s{pad(episode.season.index)}e{pad(episode.index)} -{" "}
        {episode.season.show.title}
      </Text>
      <Text variant="labelSmall">{duration(episode.totalDuration)}</Text>
    </View>
  );
}

function MovieMeta({ movie }: { movie: Movie }) {
  return (
    <View style={styles.meta}>
      <Text variant="titleLarge">{movie.title}</Text>
      <Text variant="labelSmall">{duration(movie.totalDuration)}</Text>
    </View>
  );
}

export default function VideoComponent({ video }: { video: Video }) {
  let navigation = useNavigation<NavigationProp<AppRoutes>>();

  let launchVideo = () => {
    navigation.navigate("video", {
      server: video.library.server.id,
      video: video.id,
    });
  };

  return (
    <TouchableRipple onPress={launchVideo}>
      <View style={styles.video}>
        <View style={styles.thumbContainer}>
          <Thumbnail
            style={isMovie(video) ? styles.posterThumb : styles.episodeThumb}
            thumbnail={video.thumbnail}
          />
        </View>
        {isMovie(video) ? (
          <MovieMeta movie={video} />
        ) : (
          <EpisodeMeta episode={video} />
        )}
      </View>
    </TouchableRipple>
  );
}
