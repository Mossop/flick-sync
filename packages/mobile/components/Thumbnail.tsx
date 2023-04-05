import { Image, ImageStyle, StyleProp, StyleSheet } from "react-native";
import { ThumbnailState } from "../modules/state";
import { useAppState } from "./AppState";

export default function Thumbnail({
  thumbnail,
  style,
}: {
  thumbnail: ThumbnailState;
  style?: StyleProp<ImageStyle>;
}) {
  let appState = useAppState();

  let uri =
    thumbnail.state == "downloaded" ? appState.path(thumbnail.path) : undefined;
  return <Image source={{ uri }} style={[styles.image, style]} />;
}

const styles = StyleSheet.create({
  scroller: {
    width: 1,
  },
  image: {
    width: 150,
    height: 150,
    resizeMode: "contain",
  },
});
