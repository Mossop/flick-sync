import { Image, ImageStyle, StyleProp, StyleSheet } from "react-native";
import { ThumbnailState } from "../state";
import { useSettings } from "./AppState";

const styles = StyleSheet.create({
  image: {
    width: 150,
    height: 150,
    resizeMode: "contain",
  },
});

export default function Thumbnail({
  thumbnail,
  style,
}: {
  thumbnail: ThumbnailState;
  style?: StyleProp<ImageStyle>;
}) {
  let settings = useSettings();

  let uri =
    thumbnail.state == "downloaded" ? settings.path(thumbnail.path) : undefined;
  return <Image source={{ uri }} style={[styles.image, style]} />;
}
