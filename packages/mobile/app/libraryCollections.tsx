import AppView from "../components/AppView";
import { Collection, Library } from "../state";
import Collections from "../components/Collections";
import { byTitle, useMapped } from "../modules/util";

export default function CollectionList({ library }: { library: Library }) {
  let collections = useMapped<Collection>(library.collections(), byTitle);

  return (
    <AppView title={library.title}>
      <Collections collections={collections} />
    </AppView>
  );
}
