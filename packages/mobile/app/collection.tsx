import { ScrollView } from "react-native";
import AppView from "../components/AppView";
import { useMediaState } from "../components/AppState";
import Movies from "../components/Movies";
import Shows from "../components/Shows";
import { MovieCollection, ShowCollection, isMovieCollection } from "../state";
import { AppScreenProps } from "../components/AppNavigator";
import { moviesByYear, showsByYear, useMapped } from "../modules/util";

function MovieCollectionComponent({
  collection,
}: {
  collection: MovieCollection;
}) {
  let movies = useMapped(collection.contents, moviesByYear);

  return (
    <ScrollView>
      <Movies movies={movies} />
    </ScrollView>
  );
}

function ShowCollectionComponent({
  collection,
}: {
  collection: ShowCollection;
}) {
  let shows = useMapped(collection.contents, showsByYear);

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

  let collection = mediaState
    .getServer(route.params.server)
    .getCollection(route.params.collection);
  if (!collection) {
    throw new Error("Invalid params for collection route");
  }

  return (
    <AppView title={collection.title}>
      {isMovieCollection(collection) ? (
        <MovieCollectionComponent collection={collection} />
      ) : (
        <ShowCollectionComponent collection={collection} />
      )}
    </AppView>
  );
}
