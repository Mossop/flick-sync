import { Appbar } from "react-native-paper";
import { StyleSheet, View, ViewProps } from "react-native";
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
  ...rest
}: ViewProps & {
  title: string;
}) {
  let { openDrawer } = useAppDrawer();

  return (
    <View style={styles.base}>
      <Appbar.Header>
        <Appbar.Action icon="menu" onPress={openDrawer} />
        <Appbar.Content title={title} />
      </Appbar.Header>
      <View style={[styles.base, style]} {...rest} />
    </View>
  );
}
