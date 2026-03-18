import { StyleSheet, View } from "react-native";
import { Button, Text } from "react-native-paper";
import { useState, useCallback, memo } from "react";
import { useDispatch } from "react-redux";
import { DirectMediaStore, MediaStore } from "../mediastore";
import { loadSettings, setMediaStore } from "../components/Store";

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
  let dispatch = useDispatch();
  let [error, setError] = useState<string | null>(null);

  let onChoose = useCallback(async () => {
    setError(null);
    try {
      let store = await DirectMediaStore.pickNewStore();
      await MediaStore.setCurrentStore(store);
      let settings = await loadSettings(store.location);
      dispatch(setMediaStore({ store, location: store.location, settings }));
    } catch (e) {
      setError(String(e));
    }
  }, [dispatch]);

  return (
    <View style={styles.container}>
      <Button
        mode="contained"
        onPress={() => {
          onChoose().catch(console.error);
        }}
      >
        Choose Store
      </Button>
      {error && <Text style={styles.error}>{error}</Text>}
    </View>
  );
});
