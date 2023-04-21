import { StyleSheet, View, Text } from "react-native";
import { TouchableRipple } from "react-native-paper";
import { NavigationProp, useNavigation } from "@react-navigation/native";
import { VideoState, isMovie, videoLibrary } from "../modules/state";
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

export default function Video({ video }: { video: VideoState }) {
  let navigation = useNavigation<NavigationProp<AppRoutes>>();

  let launchVideo = () => {
    navigation.navigate("video", {
      server: videoLibrary(video).server.id,
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
        <View style={styles.meta}>
          <Text>{video.title}</Text>
        </View>
      </View>
    </TouchableRipple>
  );
}
