import { memo } from "react";
import AppView from "../components/AppView";
import { LibraryState } from "../modules/state";
import Collections from "../components/Collections";

export default memo(({ library }: { library: LibraryState }) => (
  <AppView title={library.title}>
    <Collections collections={library.collections} />
  </AppView>
));
