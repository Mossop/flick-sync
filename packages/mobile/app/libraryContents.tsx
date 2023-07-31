import AppView from "../components/AppView";
import { ContainerType, Library } from "../state";
import { List, ListControls } from "../components/List";

export default function LibraryContents({ library }: { library: Library }) {
  return (
    <AppView
      title={library.title}
      actions={
        <ListControls id={library.id} container={ContainerType.Library} />
      }
    >
      <List
        id={library.id}
        container={ContainerType.Library}
        // @ts-ignore
        items={library.contents}
      />
    </AppView>
  );
}
