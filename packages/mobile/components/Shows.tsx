import { StyleSheet } from "react-native";
import { NavigationProp, useNavigation } from "@react-navigation/native";
import { TouchableRipple } from "react-native-paper";
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
          <TouchableRipple
            onPress={() =>
              navigation.navigate("show", {
                server: show.library.server.id,
                show: show.id,
              })
            }
          >
            <Thumbnail style={styles.thumb} thumbnail={show.thumbnail} />
          </TouchableRipple>
        </GridView.Item>
      ))}
    </GridView>
  );
}
