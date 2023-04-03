import { StyleSheet, View, Text } from "react-native";
import { usePlaylist } from "../../modules/util";

export default function Playlist() {
  let playlist = usePlaylist();

  return (
    <View style={styles.container}>
      <Text>{playlist.title}</Text>
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
  },
});
