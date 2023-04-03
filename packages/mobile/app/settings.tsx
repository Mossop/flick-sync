import { StyleSheet, View, Text } from "react-native";

export default function Settings() {
  return (
    <View style={styles.container}>
      <Text>settings</Text>
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
