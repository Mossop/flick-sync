import { Text } from "react-native";
import AppView from "../components/AppView";
import { LibraryState } from "../modules/state";

export default function LibraryCollections({
  library,
}: {
  library: LibraryState;
}) {
  return (
    <AppView title={library.title}>
      <Text>{library.title} collections</Text>
    </AppView>
  );
}
