import { ScrollView, StyleSheet } from "react-native";
import { MovieState } from "../modules/state";
import Thumbnail from "./Thumbnail";
import GridView from "./GridView";
import { POSTER_HEIGHT, POSTER_WIDTH } from "../modules/styles";

const styles = StyleSheet.create({
  thumb: {
    width: POSTER_WIDTH,
    height: POSTER_HEIGHT,
  },
});

export default function Movies({ movies }: { movies: MovieState[] }) {
  return (
    <ScrollView>
      <GridView itemWidth={POSTER_WIDTH}>
        {movies.map((movie) => (
          <GridView.Item key={movie.id}>
            <Thumbnail style={styles.thumb} thumbnail={movie.thumbnail} />
          </GridView.Item>
        ))}
      </GridView>
    </ScrollView>
  );
}
