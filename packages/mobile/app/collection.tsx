import { ScrollView } from "react-native";
import AppView from "../components/AppView";
import { useMediaState } from "../components/AppState";
import { isMovieCollection } from "../state";
import { AppScreenProps } from "../components/AppNavigator";
import { List, ListControls, Type } from "../components/List";

export default function Collection({ route }: AppScreenProps<"collection">) {
  let mediaState = useMediaState();
  if (!route.params) {
    throw new Error("Missing params for collection route");
  }

  let collection = mediaState
    .getServer(route.params.server)
    .getCollection(route.params.collection);
  if (!collection) {
    throw new Error("Invalid params for collection route");
  }

  let listType = isMovieCollection(collection) ? Type.Movie : Type.Show;

  return (
    <AppView
      title={collection.title}
      actions={<ListControls id={collection.id} type={listType} />}
    >
      <ScrollView>
        {/* @ts-ignore */}
        <List id={collection.id} type={listType} items={collection.contents} />
      </ScrollView>
    </AppView>
  );
}
