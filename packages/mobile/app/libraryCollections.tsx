import AppView from "../components/AppView";
import { Library } from "../state";
import { List, ListControls, Type } from "../components/List";

export default function CollectionList({ library }: { library: Library }) {
  return (
    <AppView
      title={library.title}
      actions={
        <ListControls id={`${library.id}/collections`} type={Type.Collection} />
      }
    >
      <List
        id={`${library.id}/collections`}
        type={Type.Collection}
        // @ts-ignore
        items={library.collections()}
      />
    </AppView>
  );
}
