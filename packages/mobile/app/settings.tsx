import { Text, StyleSheet } from "react-native";
import { memo } from "react";
import AppView from "../components/AppView";

const styles = StyleSheet.create({
  container: {
    alignItems: "center",
    justifyContent: "center",
  },
});

export default memo(() => (
  <AppView title="Settings" style={styles.container}>
    <Text>settings</Text>
  </AppView>
));
