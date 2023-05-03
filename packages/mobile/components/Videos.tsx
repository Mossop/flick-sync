import { StyleSheet, View } from "react-native";
import { Video } from "../state";
import VideoComponent from "./Video";
import { PADDING } from "../modules/styles";

const styles = StyleSheet.create({
  outer: {
    alignItems: "stretch",
    padding: PADDING,
  },
});

export default function Videos({ videos }: { videos: readonly Video[] }) {
  return (
    <View style={styles.outer}>
      {videos.map((video) => (
        <VideoComponent key={video.id} video={video} />
      ))}
    </View>
  );
}
