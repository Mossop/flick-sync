import { StyleSheet, View } from "react-native";
import { ActivityIndicator, Button, List } from "react-native-paper";
import { useState, useCallback, memo, use, Suspense } from "react";
import { DirectMediaStore, MediaStore } from "../mediastore";
import { UpnpMediaStore } from "../mediastore/UpnpMediaStore";
import { updateMediaStore, useSelector } from "../components/Store";
import { useActiveSearch } from "../mediastore/SsdpService";
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

function RemoteStoreItem({
  storePromise,
}: {
  storePromise: Promise<MediaStore | null>;
}) {
  let store = use(storePromise);

  if (!store) {
    return null;
  }

  return (
    <List.Item
      key={store.location}
      title={store.location}
      onPress={() => {
        updateMediaStore(store).catch(console.error);
      }}
    />
  );
}

export default memo(function MediaStorePicker() {
  useActiveSearch();
  let [error, setError] = useState<string | null>(null);
  let discoveredServers = useSelector((s) => s.discoveredServers);

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
      {error && <List.Item title={error} />}
      <Button
        mode="contained"
        onPress={() => {
          onChooseLocal().catch(console.error);
        }}
      >
        Choose Local Store
      </Button>
      <View style={styles.serverList}>
        {discoveredServers.length == 0 ? (
          <ActivityIndicator style={styles.loading} />
        ) : (
          discoveredServers.map((url) => (
            <Suspense key={url} fallback={null}>
              <RemoteStoreItem
                storePromise={UpnpMediaStore.init(url).catch(() => null)}
              />
            </Suspense>
          ))
        )}
      </View>
    </SafeAreaView>
  );
});
