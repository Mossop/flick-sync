import { StyleSheet, View, Text } from "react-native";

const styles = StyleSheet.create({
  container: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
  },
});

export default function Video() {
  return (
    <View style={styles.container}>
      <Text>video</Text>
    </View>
  );
}
