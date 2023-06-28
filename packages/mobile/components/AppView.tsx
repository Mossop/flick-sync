import { Appbar } from "react-native-paper";
import { StyleSheet, View, ViewProps } from "react-native";
import { ReactNode } from "react";
import { useAppDrawer } from "./AppNavigator";

const styles = StyleSheet.create({
  base: {
    flex: 1,
    alignItems: "stretch",
    justifyContent: "flex-start",
  },
});

export default function AppView({
  title,
  style,
  actions,
  ...rest
}: ViewProps & {
  title: string;
  actions?: ReactNode;
}) {
  let { openDrawer } = useAppDrawer();

  return (
    <View style={styles.base}>
      <Appbar.Header>
        <Appbar.Action icon="menu" onPress={openDrawer} />
        <Appbar.Content title={title} />
        {actions}
      </Appbar.Header>
      <View style={[styles.base, style]} {...rest} />
    </View>
  );
}
