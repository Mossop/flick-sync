import { Pressable, StyleSheet, View } from "react-native";
import {
  Video,
  ResizeMode,
  AVPlaybackStatusSuccess,
  AVPlaybackStatus,
} from "expo-av";
import { SafeAreaView } from "react-native-safe-area-context";
import * as StatusBar from "expo-status-bar";
import { useEffect, useRef, useState } from "react";
import * as NavigationBar from "expo-navigation-bar";
import Animated, { FadeIn, FadeOut } from "react-native-reanimated";
import { IconButton, ProgressBar, Text } from "react-native-paper";
import { useNavigation } from "@react-navigation/native";
import { useAppState } from "../components/AppState";
import { AppScreenProps } from "../components/AppNavigator";
import { isDownloaded } from "../modules/state";
import { PADDING } from "../modules/styles";

const styles = StyleSheet.create({
  container: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: "black",
  },
  video: {
    width: "100%",
    height: "100%",
  },
  overlayContainer: {
    position: "absolute",
    top: 0,
    right: 0,
    left: 0,
    bottom: 0,
    width: "100%",
    height: "100%",
  },
  overlay: {
    flex: 1,
    flexDirection: "column",
    alignItems: "stretch",
    justifyContent: "flex-start",
    padding: PADDING,
  },
  buttons: {
    flex: 1,
    flexDirection: "row",
    alignItems: "flex-start",
    justifyContent: "flex-end",
  },
  controls: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "center",
  },
  progress: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "center",
  },
  progressBar: {
    flex: 1,
    padding: PADDING,
  },
});

function useOverlayState(): [boolean, () => void] {
  let [visible, setVisible] = useState(false);
  let timeout = useRef<NodeJS.Timeout | null>(null);

  return [
    visible,
    () => {
      if (timeout.current) {
        clearTimeout(timeout.current);
      }

      if (visible) {
        setVisible(false);
      } else {
        timeout.current = setTimeout(() => {
          timeout.current = null;
          setVisible(false);
        }, 5000);
        setVisible(true);
      }
    },
  ];
}

function pad(val: number): string {
  if (val >= 10) {
    return val.toString();
  }
  return `0${val}`;
}

function time(millis: number): string {
  let secs = Math.round(millis / 1000);
  let seconds = secs % 60;
  let mins = (secs - seconds) / 60;
  let minutes = mins % 60;
  let hours = (mins - minutes) / 60;

  if (hours > 0) {
    return `${hours}:${pad(minutes)}:${pad(seconds)}`;
  }
  return `${pad(minutes)}:${pad(seconds)}`;
}

function Overlay({
  video,
  status,
  previousDuration,
  totalDuration,
}: {
  video: Video;
  status: AVPlaybackStatusSuccess;
  previousDuration: number;
  totalDuration: number;
}) {
  let navigation = useNavigation();
  let [visible, toggle] = useOverlayState();
  let position = previousDuration + status.positionMillis;

  let togglePlayback = () => {
    video.setStatusAsync({ shouldPlay: !status.isPlaying });
  };

  return (
    <Pressable style={styles.overlayContainer} onPress={toggle}>
      {visible && (
        <Animated.View
          style={styles.overlay}
          entering={FadeIn}
          exiting={FadeOut}
        >
          <View style={styles.buttons}>
            <IconButton
              icon="close"
              onPress={() => navigation.goBack()}
              iconColor="white"
              size={40}
            />
          </View>
          <View style={styles.controls}>
            {/* <IconButton icon="rewind-30" iconColor="white" size={40} />
            <IconButton icon="rewind-10" iconColor="white" size={40} /> */}
            <IconButton
              icon={status.isPlaying ? "pause" : "play"}
              onPress={togglePlayback}
              iconColor="white"
              size={80}
            />
            {/* <IconButton icon="fast-forward-10" iconColor="white" size={40} />
            <IconButton icon="fast-forward-30" iconColor="white" size={40} /> */}
          </View>
          <View style={styles.progress}>
            <Text style={{ color: "white" }}>{time(position)}</Text>
            <View style={styles.progressBar}>
              <ProgressBar
                style={{ width: "100%" }}
                progress={position / totalDuration}
              />
            </View>
            <Text style={{ color: "white" }}>
              {time(totalDuration - position)}
            </Text>
          </View>
        </Animated.View>
      )}
    </Pressable>
  );
}

export default function VideoPlayer({ route }: AppScreenProps<"video">) {
  let appState = useAppState();
  let videoRef = useRef(null);
  let [status, setStatus] = useState<AVPlaybackStatusSuccess | null>(null);

  useEffect(() => {
    NavigationBar.setVisibilityAsync("hidden");
    StatusBar.setStatusBarHidden(true, "fade");

    return () => {
      StatusBar.setStatusBarHidden(false, "fade");
      NavigationBar.setVisibilityAsync("visible");
    };
  }, []);

  if (!route.params) {
    throw new Error("Missing params for playlist route");
  }

  let video = appState.mediaState.servers
    .get(route.params.server)
    ?.videos.get(route.params.video);

  if (!video) {
    throw new Error("Incorrect params for video route");
  }

  let partIndex = route.params.part ?? 0;
  let part = video.parts[partIndex];
  let [previousDuration, totalDuration] = video.parts.reduce(
    ([previous, total], currentPart, index) => {
      if (index < partIndex) {
        return [previous + currentPart.duration, total + currentPart.duration];
      }
      return [previous, total + currentPart.duration];
    },
    [0, 0],
  );

  if (!part) {
    throw new Error("Incorrect params for video route");
  }

  let { download } = part;
  if (!isDownloaded(download)) {
    throw new Error("Unexpected missing download");
  }

  let onStatus = (avStatus: AVPlaybackStatus) => {
    if ("uri" in avStatus) {
      setStatus(avStatus);
    }
  };

  return (
    <SafeAreaView style={styles.container}>
      <Video
        ref={videoRef}
        style={styles.video}
        source={{
          uri: appState.path(download.path),
        }}
        shouldPlay
        resizeMode={ResizeMode.CONTAIN}
        onPlaybackStatusUpdate={onStatus}
      />
      {videoRef.current && status && (
        <Overlay
          previousDuration={previousDuration}
          totalDuration={totalDuration}
          status={status}
          video={videoRef.current}
        />
      )}
    </SafeAreaView>
  );
}
