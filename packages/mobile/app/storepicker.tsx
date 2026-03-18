import { StyleSheet, View } from "react-native";
import { ActivityIndicator, Button, List, Text } from "react-native-paper";
import { useState, useCallback, memo, use, useMemo, Suspense } from "react";
import { DirectMediaStore, MediaStore } from "../mediastore";
import { UpnpMediaStore } from "../mediastore/UpnpMediaStore";
import { updateMediaStore } from "../components/Store";
import { SafeAreaView } from "react-native-safe-area-context";

const styles = StyleSheet.create({
  container: {
    flex: 1,
    alignItems: "center",
    justifyContent: "flex-start",
  },
  error: {
    textAlign: "center",
  },
  loading: {
    flex: 1,
  },
  serverList: {
    flex: 1,
    width: "100%",
  },
});

function RemoteStoreList({
  storeListPromise,
}: {
  storeListPromise: Promise<MediaStore[]>;
}) {
  let stores = use(storeListPromise);

  let selectStore = useCallback((store: MediaStore) => {
    updateMediaStore(store).catch(console.error);
  }, []);

  return (
    <View style={styles.serverList}>
      {stores.map((store: MediaStore) => (
        <List.Item
          key={store.location}
          title={store.location}
          onPress={() => {
            selectStore(store);
          }}
        />
      ))}
    </View>
  );
}

export default memo(function MediaStorePicker() {
  let [error, setError] = useState<string | null>(null);

  let listRemoteStores = useMemo(() => UpnpMediaStore.listStores(), []);

  let onChooseLocal = useCallback(async () => {
    setError(null);
    try {
      let store = await DirectMediaStore.pickNewStore();
      await updateMediaStore(store);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  return (
    <SafeAreaView style={styles.container}>
      {error && <Text style={styles.error}>{error}</Text>}
      <Button
        mode="contained"
        onPress={() => {
          onChooseLocal().catch(console.error);
        }}
      >
        Choose Local Store
      </Button>
      <Suspense fallback={<ActivityIndicator style={styles.loading} />}>
        <RemoteStoreList storeListPromise={listRemoteStores} />
      </Suspense>
    </SafeAreaView>
  );
});
