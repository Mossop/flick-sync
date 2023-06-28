import { NavigationProp, useNavigation } from "@react-navigation/native";
import { TouchableRipple } from "react-native-paper";
import { Show } from "../state";
import GridView from "./GridView";
import { POSTER_WIDTH } from "../modules/styles";
import { AppRoutes } from "./AppNavigator";
import Poster from "./Poster";

export default function Shows({ shows }: { shows: Show[] }) {
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
            <Poster text={show.title} thumbnail={show.thumbnail} />
          </TouchableRipple>
        </GridView.Item>
      ))}
    </GridView>
  );
}
