import { useState } from "react";
import { LayoutChangeEvent, StyleSheet, View } from "react-native";
import { Text, useTheme } from "react-native-paper";
import { PADDING } from "../modules/styles";

const BAR_HEIGHT = 6;
const SCRUBBER_SIZE = BAR_HEIGHT * 3;

const styles = StyleSheet.create({
  root: {
    width: "100%",
    flexDirection: "column",
    alignItems: "stretch",
    justifyContent: "flex-start",
  },
  labels: {
    flexDirection: "row",
    justifyContent: "space-between",
    paddingStart: PADDING,
    paddingEnd: PADDING,
  },
  progressbar: {
    flexDirection: "row",
    paddingTop: (SCRUBBER_SIZE - BAR_HEIGHT) / 2,
    paddingBottom: (SCRUBBER_SIZE - BAR_HEIGHT) / 2,
  },
  barchunk: {
    height: BAR_HEIGHT,
  },
  scrubber: {
    position: "absolute",
    top: 0,
    borderRadius: SCRUBBER_SIZE / 2,
    width: SCRUBBER_SIZE,
    height: SCRUBBER_SIZE,
    marginLeft: -(SCRUBBER_SIZE / 2),
  },
});

export interface ScrubberProps {
  position: number;
  totalDuration: number;
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

export default function Scrubber({ position, totalDuration }: ScrubberProps) {
  let theme = useTheme();
  let [fullWidth, setWidth] = useState(0);

  let color = theme.colors.primary;
  let background = theme.colors.surfaceVariant;

  let progressWidth = Math.round((fullWidth * position) / totalDuration);

  let onLayout = (event: LayoutChangeEvent) => {
    setWidth(event.nativeEvent.layout.width);
  };

  return (
    <View style={styles.root} onLayout={onLayout}>
      <View style={styles.progressbar}>
        <View
          style={[
            styles.barchunk,
            { width: progressWidth, backgroundColor: color },
          ]}
        />
        <View
          style={[styles.barchunk, { flex: 1, backgroundColor: background }]}
        />
      </View>
      <View
        style={[
          styles.scrubber,
          { left: progressWidth, backgroundColor: color },
        ]}
      />
      <View style={styles.labels}>
        <Time value={position} />
        <Time value={totalDuration - position} />
      </View>
    </View>
  );
}
