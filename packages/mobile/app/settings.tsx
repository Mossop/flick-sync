import { Text, StyleSheet } from "react-native";
import AppView from "../components/AppView";

export default function Settings() {
  return (
    <AppView title="Settings" style={styles.container}>
      <Text>settings</Text>
    </AppView>
  );
}

const styles = StyleSheet.create({
  container: {
    alignItems: "center",
    justifyContent: "center",
  },
});
