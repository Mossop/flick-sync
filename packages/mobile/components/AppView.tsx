import { Appbar } from "react-native-paper";
import { useAppDrawer } from "./AppNavigator";
import { StyleSheet, View, ViewProps } from "react-native";

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

const styles = StyleSheet.create({
  base: {
    flex: 1,
  },
});
