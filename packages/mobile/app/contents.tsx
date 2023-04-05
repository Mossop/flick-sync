import { Text } from "react-native";
import AppView from "../components/AppView";
import { LibraryState, isMovieLibrary } from "../modules/state";
import Movies from "../components/Movies";
import Shows from "../components/Shows";
import { memo } from "react";

export default memo(function LibraryContent({
  library,
}: {
  library: LibraryState;
}) {
  return (
    <AppView title={library.title}>
      {isMovieLibrary(library) ? (
        <Movies movies={library.contents} />
      ) : (
        <Shows shows={library.contents} />
      )}
    </AppView>
  );
});
