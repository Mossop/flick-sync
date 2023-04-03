import { StyleSheet, View, Text } from "react-native";

export default function Library() {
  return (
    <View style={styles.container}>
      <Text>library</Text>
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
