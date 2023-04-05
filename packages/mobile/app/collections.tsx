import { Text } from "react-native";
import { memo } from "react";
import AppView from "../components/AppView";
import { LibraryState } from "../modules/state";

export default memo(({ library }: { library: LibraryState }) => (
  <AppView title={library.title}>
    <Text>{library.title} collections</Text>
  </AppView>
));
