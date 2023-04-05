import { memo } from "react";
import { ScrollView } from "react-native";
import AppView from "../components/AppView";
import { LibraryState, isMovieLibrary } from "../modules/state";
import Movies from "../components/Movies";
import Shows from "../components/Shows";

export default memo(({ library }: { library: LibraryState }) => (
  <AppView title={library.title}>
    <ScrollView>
      {isMovieLibrary(library) ? (
        <Movies movies={library.contents} />
      ) : (
        <Shows shows={library.contents} />
      )}
    </ScrollView>
  </AppView>
));
