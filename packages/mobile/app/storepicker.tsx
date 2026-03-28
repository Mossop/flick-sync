import { ScrollView, StyleSheet } from "react-native";
import { List } from "react-native-paper";
import { useCallback, memo, use, Suspense } from "react";
import { DirectMediaStore, MediaStore } from "../mediastore";
import { UpnpMediaStore } from "../mediastore/UpnpMediaStore";
import {
  updateMediaStore,
  useAction,
  useSelector,
  reportError,
} from "../components/Store";
import { useActiveSearch } from "../mediastore/SsdpService";
import { SafeAreaView } from "react-native-safe-area-context";
import { MaterialCommunityIcons, MaterialIcons } from "@expo/vector-icons";

const styles = StyleSheet.create({
  container: {
    flex: 1,
    alignItems: "stretch",
    justifyContent: "flex-start",
  },
  list: {
    flex: 1,
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
      left={(props) => (
        <MaterialCommunityIcons {...props} size={24} name="server" />
      )}
      title={store.location}
      onPress={() => {
        updateMediaStore(store).catch(console.error);
      }}
    />
  );
}

export default memo(function MediaStorePicker() {
  useActiveSearch();
  let setError = useAction(reportError);
  let discoveredServers = useSelector((s) => s.discoveredServers);

  let onChooseLocal = useCallback(async () => {
    try {
      let store = await DirectMediaStore.pickNewStore();
      await updateMediaStore(store);
    } catch (e) {
      setError(String(e));
    }
  }, [setError]);

  return (
    <SafeAreaView style={styles.container}>
      <ScrollView>
        <List.Item
          left={(props) => <MaterialIcons {...props} size={24} name="folder" />}
          onPress={() => {
            onChooseLocal().catch(console.error);
          }}
          title="Choose local store"
        />
        {discoveredServers.map((url) => (
          <Suspense key={url} fallback={null}>
            <RemoteStoreItem
              storePromise={UpnpMediaStore.init(url).catch(() => null)}
            />
          </Suspense>
        ))}
      </ScrollView>
    </SafeAreaView>
  );
});
