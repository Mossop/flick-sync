import AppView from "../components/AppView";
import { Library, isMovieLibrary } from "../state";
import { List, ListControls, Type } from "../components/List";

export default function LibraryContents({ library }: { library: Library }) {
  let listType = isMovieLibrary(library) ? Type.Movie : Type.Show;

  return (
    <AppView
      title={library.title}
      actions={<ListControls id={library.id} type={listType} />}
    >
      {/* @ts-ignore */}
      <List id={library.id} type={listType} items={library.contents} />
    </AppView>
  );
}
