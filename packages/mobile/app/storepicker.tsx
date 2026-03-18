import { StyleSheet, View } from "react-native";
import { Button, Text } from "react-native-paper";
import { useState, useCallback, memo } from "react";
import { DirectMediaStore } from "../mediastore";
import { updateMediaStore } from "../components/Store";

const styles = StyleSheet.create({
  container: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
  },
  error: {
    marginTop: 16,
    textAlign: "center",
  },
});

export default memo(function MediaStorePicker() {
  let [error, setError] = useState<string | null>(null);

  let onChoose = useCallback(async () => {
    setError(null);
    try {
      let store = await DirectMediaStore.pickNewStore();
      await updateMediaStore(store);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  return (
    <View style={styles.container}>
      <Button
        mode="contained"
        onPress={() => {
          onChoose().catch(console.error);
        }}
      >
        Choose Local Store
      </Button>
      {error && <Text style={styles.error}>{error}</Text>}
    </View>
  );
});
