import { StyleSheet, View } from "react-native";
import { VideoState } from "../modules/state";
import Video from "./Video";
import { PADDING } from "../modules/styles";

const styles = StyleSheet.create({
  outer: {
    alignItems: "stretch",
    padding: PADDING,
  },
});

export default function Videos({ videos }: { videos: VideoState[] }) {
  return (
    <View style={styles.outer}>
      {videos.map((video) => (
        <Video key={video.id} video={video} />
      ))}
    </View>
  );
}
