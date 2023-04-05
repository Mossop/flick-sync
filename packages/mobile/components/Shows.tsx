import { ShowState } from "../modules/state";
import { ScrollView, StyleSheet } from "react-native";
import Thumbnail from "./Thumbnail";
import GridView from "./GridView";
import { POSTER_HEIGHT, POSTER_WIDTH } from "../modules/styles";

export default function Shows({ shows }: { shows: ShowState[] }) {
  return (
    <ScrollView>
      <GridView itemWidth={POSTER_WIDTH}>
        {shows.map((show) => (
          <GridView.Item key={show.id}>
            <Thumbnail style={styles.thumb} thumbnail={show.thumbnail} />
          </GridView.Item>
        ))}
      </GridView>
    </ScrollView>
  );
}

const styles = StyleSheet.create({
  thumb: {
    width: POSTER_WIDTH,
    height: POSTER_HEIGHT,
  },
});
