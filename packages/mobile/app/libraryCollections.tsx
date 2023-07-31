import AppView from "../components/AppView";
import { ContainerType, Library } from "../state";
import { List, ListControls } from "../components/List";

export default function CollectionList({ library }: { library: Library }) {
  return (
    <AppView
      title={library.title}
      actions={
        <ListControls
          id={`${library.id}/collections`}
          container={ContainerType.Library}
        />
      }
    >
      <List
        id={`${library.id}/collections`}
        container={ContainerType.Library}
        // @ts-ignore
        items={library.collections()}
      />
    </AppView>
  );
}
