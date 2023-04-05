import { ScrollView, StyleSheet } from "react-native";
import { CollectionState } from "../modules/state";
import Thumbnail from "./Thumbnail";
import GridView from "./GridView";
import { POSTER_HEIGHT, POSTER_WIDTH } from "../modules/styles";

const styles = StyleSheet.create({
  thumb: {
    width: POSTER_WIDTH,
    height: POSTER_HEIGHT,
  },
});

export default function Collections({
  collections,
}: {
  collections: CollectionState[];
}) {
  return (
    <ScrollView>
      <GridView itemWidth={POSTER_WIDTH}>
        {collections.map((collection) => (
          <GridView.Item key={collection.id}>
            <Thumbnail style={styles.thumb} thumbnail={collection.thumbnail} />
          </GridView.Item>
        ))}
      </GridView>
    </ScrollView>
  );
}
