import { StyleSheet } from "react-native";
import { TouchableRipple } from "react-native-paper";
import { NavigationProp, useNavigation } from "@react-navigation/native";
import { MovieState, VideoState, videoLibrary } from "../modules/state";
import Thumbnail from "./Thumbnail";
import GridView from "./GridView";
import { POSTER_HEIGHT, POSTER_WIDTH } from "../modules/styles";
import { AppRoutes } from "./AppNavigator";

const styles = StyleSheet.create({
  thumb: {
    width: POSTER_WIDTH,
    height: POSTER_HEIGHT,
  },
});

export default function Movies({ movies }: { movies: MovieState[] }) {
  let navigation = useNavigation<NavigationProp<AppRoutes>>();

  let launchVideo = (video: VideoState) => {
    navigation.navigate("video", {
      server: videoLibrary(video).server.id,
      video: video.id,
    });
  };

  return (
    <GridView itemWidth={POSTER_WIDTH}>
      {movies.map((movie) => (
        <GridView.Item key={movie.id}>
          <TouchableRipple onPress={() => launchVideo(movie)}>
            <Thumbnail style={styles.thumb} thumbnail={movie.thumbnail} />
          </TouchableRipple>
        </GridView.Item>
      ))}
    </GridView>
  );
}
