import AppView from "../components/AppView";
import { useMediaState } from "../modules/util";
import { ContainerType, isMovieCollection } from "../state";
import { AppScreenProps } from "../components/AppNavigator";
import { List, ListControls } from "../components/List";

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

  let container = isMovieCollection(collection)
    ? ContainerType.MovieCollection
    : ContainerType.ShowCollection;

  return (
    <AppView
      title={collection.title}
      actions={<ListControls id={collection.id} container={container} />}
    >
      <List
        id={collection.id}
        container={container}
        // @ts-ignore
        items={collection.contents}
      />
    </AppView>
  );
}
