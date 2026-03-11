import { useCallback, useMemo, useState } from "react";
import { LayoutChangeEvent, StyleSheet, View } from "react-native";
import { Text, useTheme } from "react-native-paper";
import { Gesture, GestureDetector } from "react-native-gesture-handler";
import Animated, {
  useAnimatedStyle,
  useDerivedValue,
  useSharedValue,
} from "react-native-reanimated";
import { PADDING } from "../modules/styles";
import { scheduleOnRN } from "react-native-worklets";

const BAR_HEIGHT = 6;
const SCRUBBER_SIZE = BAR_HEIGHT * 3;

const styles = StyleSheet.create({
  root: {
    width: "100%",
    flexDirection: "column",
    alignItems: "stretch",
    justifyContent: "flex-start",
    marginHorizontal: PADDING,
    paddingVertical: PADDING,
  },
  labels: {
    flexDirection: "row",
    justifyContent: "space-between",
    paddingHorizontal: PADDING,
  },
  progressbar: {
    flexDirection: "row",
    padding: (SCRUBBER_SIZE - BAR_HEIGHT) / 2,
  },
  barchunk: {
    height: BAR_HEIGHT,
  },
  scrubber: {
    position: "absolute",
    top: PADDING,
    borderRadius: SCRUBBER_SIZE / 2,
    width: SCRUBBER_SIZE,
    height: SCRUBBER_SIZE,
    marginLeft: -(SCRUBBER_SIZE / 2),
  },
});

export interface ScrubberProps {
  position: number;
  nextPosition?: number;
  totalDuration: number;
  onScrubbing: (position: number) => void;
  onScrubbingComplete: (position: number) => void;
}

function pad(val: number): string {
  if (val >= 10) {
    return val.toString();
  }
  return `0${val}`;
}

function time(millis: number): string {
  let secs = Math.round(millis / 1000);
  let hours = Math.floor(secs / 3600);
  let minutes = Math.floor(secs / 60) % 60;
  let seconds = secs % 60;

  if (hours > 0) {
    return `${hours}:${pad(minutes)}:${pad(seconds)}`;
  }
  return `${pad(minutes)}:${pad(seconds)}`;
}

function Time({ value }: { value: number }) {
  let theme = useTheme();

  return (
    <Text
      style={{
        color: theme.colors.onBackground,
      }}
    >
      {time(value)}
    </Text>
  );
}

export default function Scrubber({
  position,
  nextPosition,
  totalDuration,
  onScrubbing,
  onScrubbingComplete,
}: ScrubberProps) {
  let theme = useTheme();
  let [fullWidth, setWidth] = useState(0);

  let filledColor = theme.colors.primary;
  let unfilledColor = theme.colors.surfaceVariant;

  let progressWidth = Math.round((fullWidth * position) / totalDuration);

  let onLayout = (event: LayoutChangeEvent) => {
    setWidth(event.nativeEvent.layout.width);
  };

  let selectedPosition = useSharedValue<number | null>(null);
  let displayPosition = useDerivedValue(
    () => selectedPosition.value ?? position,
    [position],
  );

  let finishScrubbing = useCallback(
    (value: number) => {
      onScrubbingComplete(value);
      selectedPosition.value = null;
    },
    [selectedPosition, onScrubbingComplete],
  );

  let panGesture = useMemo(
    () =>
      Gesture.Pan()
        .onStart((event) => {
          selectedPosition.value = Math.round(
            (totalDuration * event.x) / fullWidth,
          );
        })
        .onUpdate((event) => {
          let nextPosition = Math.round((totalDuration * event.x) / fullWidth);
          selectedPosition.value = nextPosition;
          scheduleOnRN(onScrubbing, nextPosition);
        })
        .onEnd(() => {
          if (selectedPosition.value !== null) {
            scheduleOnRN(finishScrubbing, selectedPosition.value);
          }
        }),
    [fullWidth, totalDuration, onScrubbing, finishScrubbing, selectedPosition],
  );

  let animatedStyle = useAnimatedStyle(() => ({
    left: Math.round((fullWidth * displayPosition.value) / totalDuration),
  }));

  return (
    <GestureDetector gesture={panGesture}>
      <View style={styles.root} onLayout={onLayout}>
        <View style={styles.progressbar}>
          <View
            style={[
              styles.barchunk,
              { width: progressWidth, backgroundColor: filledColor },
            ]}
          />
          <View
            style={[
              styles.barchunk,
              { flex: 1, backgroundColor: unfilledColor },
            ]}
          />
        </View>
        <Animated.View
          style={[
            styles.scrubber,
            { backgroundColor: filledColor },
            animatedStyle,
          ]}
        />
        <View style={styles.labels}>
          <Time value={nextPosition ?? position} />
          <Time value={totalDuration - (nextPosition ?? position)} />
        </View>
      </View>
    </GestureDetector>
  );
}
