import { StyleSheet, View, Text } from "react-native";
import { useLibrary } from "../../../modules/util";

export default function Collections() {
  let library = useLibrary();

  return (
    <View style={styles.container}>
      <Text>{library.title} collections</Text>
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
  },
});
