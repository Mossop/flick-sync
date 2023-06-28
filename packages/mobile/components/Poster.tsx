import { StyleSheet, View } from "react-native";
import { Text } from "react-native-paper";
import { ThumbnailState } from "../state";
import Thumbnail from "./Thumbnail";
import { PADDING, POSTER_HEIGHT, POSTER_WIDTH } from "../modules/styles";

const styles = StyleSheet.create({
  container: {
    flexDirection: "column",
    alignItems: "center",
    gap: PADDING,
  },
  title: {
    textAlign: "center",
  },
  thumb: {
    width: POSTER_WIDTH,
    height: POSTER_HEIGHT,
  },
});

export default function Poster({
  thumbnail,
  text,
}: {
  thumbnail: ThumbnailState;
  text: string;
}) {
  return (
    <View style={styles.container}>
      <Thumbnail style={styles.thumb} thumbnail={thumbnail} />
      <Text
        variant="labelSmall"
        style={styles.title}
        numberOfLines={1}
        ellipsizeMode="tail"
      >
        {text}
      </Text>
    </View>
  );
}
