import { ScrollView } from "react-native";
import AppView from "../components/AppView";
import {
  LibraryState,
  MovieLibraryState,
  ShowLibraryState,
  isMovieLibrary,
} from "../modules/state";
import Movies from "../components/Movies";
import Shows from "../components/Shows";
import { useMapped, byTitle } from "../modules/util";

function MovieLibraryContents({ library }: { library: MovieLibraryState }) {
  let movies = useMapped(library.contents, byTitle);

  return (
    <ScrollView>
      <Movies movies={movies} />
    </ScrollView>
  );
}

function ShowLibraryContents({ library }: { library: ShowLibraryState }) {
  let shows = useMapped(library.contents, byTitle);

  return (
    <ScrollView>
      <Shows shows={shows} />
    </ScrollView>
  );
}

export default function LibraryContents({
  library,
}: {
  library: LibraryState;
}) {
  return (
    <AppView title={library.title}>
      <ScrollView>
        {isMovieLibrary(library) ? (
          <MovieLibraryContents library={library} />
        ) : (
          <ShowLibraryContents library={library} />
        )}
      </ScrollView>
    </AppView>
  );
}
