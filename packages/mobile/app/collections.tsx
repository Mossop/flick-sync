import AppView from "../components/AppView";
import { CollectionState, LibraryState } from "../modules/state";
import Collections from "../components/Collections";
import { byTitle, useMapped } from "../modules/util";

export default function CollectionList({ library }: { library: LibraryState }) {
  let collections = useMapped<CollectionState>(library.collections, byTitle);

  return (
    <AppView title={library.title}>
      <Collections collections={collections} />
    </AppView>
  );
}
