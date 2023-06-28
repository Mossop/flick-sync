import { TouchableRipple } from "react-native-paper";
import { NavigationProp, useNavigation } from "@react-navigation/native";
import { Movie, Video } from "../state";
import GridView from "./GridView";
import { POSTER_WIDTH } from "../modules/styles";
import { AppRoutes } from "./AppNavigator";
import Poster from "./Poster";

export default function Movies({ movies }: { movies: Movie[] }) {
  let navigation = useNavigation<NavigationProp<AppRoutes>>();

  let launchVideo = (video: Video) => {
    navigation.navigate("video", {
      server: video.library.server.id,
      video: video.id,
    });
  };

  return (
    <GridView itemWidth={POSTER_WIDTH}>
      {movies.map((movie) => (
        <GridView.Item key={movie.id}>
          <TouchableRipple onPress={() => launchVideo(movie)}>
            <Poster thumbnail={movie.thumbnail} text={movie.title} />
          </TouchableRipple>
        </GridView.Item>
      ))}
    </GridView>
  );
}
