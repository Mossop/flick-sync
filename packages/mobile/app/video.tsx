import { StyleSheet, View, Text } from "react-native";

export default function Video() {
  return (
    <View style={styles.container}>
      <Text>video</Text>
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
