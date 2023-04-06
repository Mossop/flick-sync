import { StyleSheet, View } from "react-native";
import { EpisodeState } from "../modules/state";
import Video from "./Video";
import { PADDING } from "../modules/styles";

const styles = StyleSheet.create({
  outer: {
    alignItems: "stretch",
    padding: PADDING,
  },
});

export default function Episodes({ episodes }: { episodes: EpisodeState[] }) {
  return (
    <View style={styles.outer}>
      {episodes.map((episode) => (
        <Video key={episode.id} video={episode} />
      ))}
    </View>
  );
}
