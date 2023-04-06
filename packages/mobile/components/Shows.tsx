import { Pressable, StyleSheet } from "react-native";
import { NavigationProp, useNavigation } from "@react-navigation/native";
import { ShowState } from "../modules/state";
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

export default function Shows({ shows }: { shows: ShowState[] }) {
  let navigation = useNavigation<NavigationProp<AppRoutes>>();

  return (
    <GridView itemWidth={POSTER_WIDTH}>
      {shows.map((show) => (
        <GridView.Item key={show.id}>
          <Pressable
            onPress={() =>
              navigation.navigate("show", {
                server: show.library.server.id,
                show: show.id,
              })
            }
          >
            <Thumbnail style={styles.thumb} thumbnail={show.thumbnail} />
          </Pressable>
        </GridView.Item>
      ))}
    </GridView>
  );
}
