import { ScrollView } from "react-native";
import AppView from "../components/AppView";
import { useMediaState } from "../components/AppState";
import Movies from "../components/Movies";
import Shows from "../components/Shows";
import {
  MovieCollectionState,
  ShowCollectionState,
  isMovieCollection,
} from "../modules/state";
import { AppScreenProps } from "../components/AppNavigator";
import { moviesByYear, showsByYear, useMapped } from "../modules/util";

function MovieCollection({ collection }: { collection: MovieCollectionState }) {
  let movies = useMapped(collection.items, moviesByYear);

  return (
    <ScrollView>
      <Movies movies={movies} />
    </ScrollView>
  );
}

function ShowCollection({ collection }: { collection: ShowCollectionState }) {
  let shows = useMapped(collection.items, showsByYear);

  return (
    <ScrollView>
      <Shows shows={shows} />
    </ScrollView>
  );
}

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
      {isMovieCollection(collection) ? (
        <MovieCollection collection={collection} />
      ) : (
        <ShowCollection collection={collection} />
      )}
    </AppView>
  );
}
