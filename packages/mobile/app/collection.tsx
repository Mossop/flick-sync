import { ScrollView } from "react-native";
import AppView from "../components/AppView";
import { useMediaState } from "../components/AppState";
import Movies from "../components/Movies";
import Shows from "../components/Shows";
import { isMovieCollection } from "../modules/state";
import { AppScreenProps } from "../components/AppNavigator";

export default function Collection({ route }: AppScreenProps<"collection">) {
  let mediaState = useMediaState();
  if (!route.params) {
    throw new Error("Missing params for collection route");
  }

  let collection = mediaState.servers
    .get(route.params.server)
    ?.collections.get(route.params.collection);
  if (!collection) {
    throw new Error("Invalid params for collection route");
  }

  return (
    <AppView title={collection.title}>
      <ScrollView>
        {isMovieCollection(collection) ? (
          <Movies movies={collection.items} />
        ) : (
          <Shows shows={collection.items} />
        )}
      </ScrollView>
    </AppView>
  );
}
