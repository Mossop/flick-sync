import { Pressable, ScrollView, StyleSheet } from "react-native";
import { NavigationProp, useNavigation } from "@react-navigation/native";
import { CollectionState } from "../modules/state";
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

export default function Collections({
  collections,
}: {
  collections: CollectionState[];
}) {
  let navigation = useNavigation<NavigationProp<AppRoutes>>();

  return (
    <ScrollView>
      <GridView itemWidth={POSTER_WIDTH}>
        {collections.map((collection) => (
          <GridView.Item key={collection.id}>
            <Pressable
              onPress={() =>
                navigation.navigate("collection", {
                  server: collection.library.server.id,
                  collection: collection.id,
                })
              }
            >
              <Thumbnail
                style={styles.thumb}
                thumbnail={collection.thumbnail}
              />
            </Pressable>
          </GridView.Item>
        ))}
      </GridView>
    </ScrollView>
  );
}
