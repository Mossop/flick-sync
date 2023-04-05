import { Text } from "react-native";
import AppView from "../components/AppView";
import { LibraryState } from "../modules/state";
import { memo } from "react";

export default memo(function LibraryCollections({
  library,
}: {
  library: LibraryState;
}) {
  return (
    <AppView title={library.title}>
      <Text>{library.title} collections</Text>
    </AppView>
  );
});
