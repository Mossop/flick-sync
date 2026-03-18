import { StyleSheet, View } from "react-native";
import { Text, TouchableRipple } from "react-native-paper";
import { ReactNode, memo, useCallback } from "react";
import AppView from "../components/AppView";
import { PADDING } from "../modules/styles";
import {
  reportError,
  setStoreLocation,
  useAction,
  useSelector,
  loadSettings,
} from "../components/Store";
import { useMediaStore } from "../store";

const styles = StyleSheet.create({
  container: {
    alignItems: "stretch",
    justifyContent: "flex-start",
  },
  block: {
    padding: PADDING * 2,
  },
});

function SettingBlock({
  title,
  onPress,
  children,
}: {
  title: string;
  onPress?: () => void;
  children: ReactNode;
}) {
  return (
    <TouchableRipple onPress={onPress}>
      <View style={styles.block}>
        <Text variant="titleMedium">{title}</Text>
        {children}
      </View>
    </TouchableRipple>
  );
}

export default memo(function Settings() {
  let storeLocation = useSelector((storeState) => storeState.storeLocation);
  let mediaStore = useMediaStore();
  let dispatchSetStoreLocation = useAction(setStoreLocation);
  let dispatchReportError = useAction(reportError);

  let onPickNew = useCallback(async () => {
    try {
      await mediaStore.pickNew();
      let settings = await loadSettings(mediaStore.location);
      dispatchSetStoreLocation({ location: mediaStore.location, settings });
    } catch (e) {
      dispatchReportError(String(e));
    }
  }, [mediaStore, dispatchSetStoreLocation, dispatchReportError]);

  return (
    <AppView title="Settings" style={styles.container}>
      <SettingBlock
        title="Store"
        onPress={() => {
          onPickNew().catch(console.error);
        }}
      >
        <View style={{ flexDirection: "row" }}>
          <Text style={{ flex: 1 }}>{storeLocation}</Text>
        </View>
      </SettingBlock>
    </AppView>
  );
});
