import { ScrollView } from "react-native";
import { NavigationProp, useNavigation } from "@react-navigation/native";
import { TouchableRipple } from "react-native-paper";
import { Collection } from "../state";
import GridView from "./GridView";
import { POSTER_WIDTH } from "../modules/styles";
import { AppRoutes } from "./AppNavigator";
import Poster from "./Poster";

export default function Collections({
  collections,
}: {
  collections: Collection[];
}) {
  let navigation = useNavigation<NavigationProp<AppRoutes>>();

  return (
    <ScrollView>
      <GridView itemWidth={POSTER_WIDTH}>
        {collections.map((collection) => (
          <GridView.Item key={collection.id}>
            <TouchableRipple
              onPress={() =>
                navigation.navigate("collection", {
                  server: collection.library.server.id,
                  collection: collection.id,
                })
              }
            >
              <Poster
                thumbnail={collection.thumbnail}
                text={collection.title}
              />
            </TouchableRipple>
          </GridView.Item>
        ))}
      </GridView>
    </ScrollView>
  );
}
