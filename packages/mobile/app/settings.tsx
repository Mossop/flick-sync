import { StyleSheet, View } from "react-native";
import { Text, TouchableRipple } from "react-native-paper";
import { ReactNode, memo } from "react";
import AppView from "../components/AppView";
import { PADDING } from "../modules/styles";
import { useSettings } from "../components/AppState";

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

export default memo(() => {
  let settings = useSettings();

  return (
    <AppView title="Settings" style={styles.container}>
      <SettingBlock title="Store" onPress={() => settings.pickStore()}>
        <View style={{ flexDirection: "row" }}>
          <Text style={{ flex: 1 }}>{settings.store}</Text>
        </View>
      </SettingBlock>
    </AppView>
  );
});
