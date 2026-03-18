import AppView from "../components/AppView";
import { ContainerType, isMovieCollection } from "../state";
import { AppScreenProps } from "../components/AppNavigator";
import { List, ListControls } from "../components/List";
import { useCollection } from "../mediastore";

export default function Collection({ route }: AppScreenProps<"collection">) {
  if (!route.params) {
    throw new Error("Missing params for collection route");
  }

  let collection = useCollection(route.params.server, route.params.collection);

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
        // @ts-expect-error
        items={collection.contents}
        inset={true}
      />
    </AppView>
  );
}
