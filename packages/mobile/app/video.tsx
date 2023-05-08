import { Pressable, StyleSheet, View } from "react-native";
import {
  Video,
  ResizeMode,
  AVPlaybackStatusSuccess,
  AVPlaybackStatus,
} from "expo-av";
import { SafeAreaView } from "react-native-safe-area-context";
import * as StatusBar from "expo-status-bar";
import { useCallback, useEffect, useRef, useState } from "react";
import * as NavigationBar from "expo-navigation-bar";
import Animated, { FadeIn, FadeOut } from "react-native-reanimated";
import { IconButton, useTheme } from "react-native-paper";
import { useNavigation } from "@react-navigation/native";
import { activateKeepAwakeAsync, deactivateKeepAwake } from "expo-keep-awake";
import * as ScreenOrientation from "expo-screen-orientation";
import { OrientationLock } from "expo-screen-orientation";
import { useMediaState, useSettings } from "../components/AppState";
import { AppScreenProps } from "../components/AppNavigator";
import { PADDING } from "../modules/styles";
import Scrubber from "../components/Scrubber";
import { SchemeOverride } from "../components/ThemeProvider";
import { isDownloaded } from "../state";

const styles = StyleSheet.create({
  container: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
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
        // timeout.current = setTimeout(() => {
        //   timeout.current = null;
        //   setVisible(false);
        // }, 5000);
        setVisible(true);
      }
    },
  ];
}

function Overlay({
  seek,
  video,
  status,
  previousDuration,
  totalDuration,
}: {
  seek: (position: number) => void;
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

  let skip = (delta: number) => seek(position + delta);

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
              size={40}
            />
          </View>
          <View style={styles.controls}>
            <IconButton
              icon="rewind-30"
              onPress={() => skip(-30000)}
              size={40}
            />
            <IconButton
              icon="rewind-10"
              onPress={() => skip(-15000)}
              size={40}
            />
            <IconButton
              icon={status.isPlaying ? "pause" : "play"}
              onPress={togglePlayback}
              size={80}
            />
            <IconButton
              icon="fast-forward-10"
              onPress={() => skip(15000)}
              size={40}
            />
            <IconButton
              icon="fast-forward-30"
              onPress={() => skip(30000)}
              size={40}
            />
          </View>
          <Scrubber
            position={position}
            totalDuration={totalDuration}
            onScrubbingComplete={seek}
          />
        </Animated.View>
      )}
    </Pressable>
  );
}

export default function VideoPlayer({ route }: AppScreenProps<"video">) {
  let navigation = useNavigation();
  let settings = useSettings();
  let mediaState = useMediaState();
  let videoRef = useRef<Video | null>(null);
  let [playbackStatus, setPlaybackStatus] =
    useState<AVPlaybackStatusSuccess | null>(null);
  let theme = useTheme();
  let initialized = useRef(false);

  let video = mediaState
    .getServer(route.params.server)
    .getVideo(route.params.video);

  let playbackPosition = useRef<number | undefined>(video.playPosition);

  let previousDuration = useRef<number>(0);
  let currentPart = useRef<number>();
  let seek = useCallback(
    async (position: number) => {
      let targetPart = 0;
      let partPosition = Math.min(Math.max(position, 0), video.totalDuration);

      while (
        targetPart < video.parts.length - 1 &&
        video.parts[targetPart]!.duration >= position
      ) {
        partPosition -= video.parts[targetPart]!.duration;
        targetPart++;
      }

      if (targetPart === currentPart.current) {
        videoRef.current!.playFromPositionAsync(partPosition);
      } else {
        await videoRef.current!.unloadAsync();

        currentPart.current = targetPart;
        let { download } = video.parts[targetPart]!;

        if (!isDownloaded(download)) {
          throw new Error("Unexpected non-downloaded part.");
        }

        console.log(`Loading ${download.path} at position ${partPosition}`);
        await videoRef.current!.loadAsync(
          { uri: settings.path(download.path) },
          { positionMillis: partPosition, shouldPlay: true },
        );
      }
    },
    [video, settings],
  );

  useEffect(() => {
    NavigationBar.setVisibilityAsync("hidden");
    StatusBar.setStatusBarHidden(true, "fade");
    ScreenOrientation.lockAsync(OrientationLock.LANDSCAPE);

    return () => {
      StatusBar.setStatusBarHidden(false, "fade");
      NavigationBar.setVisibilityAsync("visible");
      ScreenOrientation.unlockAsync();
      deactivateKeepAwake();
    };
  }, []);

  useEffect((): (() => void) | undefined => {
    if (playbackStatus?.isPlaying) {
      activateKeepAwakeAsync();
      return () => {
        deactivateKeepAwake();
      };
    }

    return undefined;
  }, [playbackStatus?.isPlaying]);

  useEffect(() => {
    if (!initialized.current) {
      seek(video.playPosition ?? 0);
      initialized.current = true;
    }
  }, [video, seek]);

  let onStatus = useCallback(
    (avStatus: AVPlaybackStatus) => {
      if ("uri" in avStatus) {
        setPlaybackStatus(avStatus);
        playbackPosition.current =
          previousDuration.current + avStatus.positionMillis;

        if (avStatus.didJustFinish) {
          if (currentPart.current == video.parts.length - 1) {
            video.playPosition = undefined;
            navigation.goBack();
          } else {
            previousDuration.current +=
              video.parts[currentPart.current!]!.duration;
            currentPart.current = currentPart.current! + 1;
            playbackPosition.current = previousDuration.current;
            video.playPosition = previousDuration.current;

            let { download } = video.parts[currentPart.current!]!;
            if (!isDownloaded(download)) {
              throw new Error("Unexpected non-downloaded part.");
            }

            videoRef.current?.loadAsync(
              { uri: settings.path(download.path) },
              { positionMillis: 0, shouldPlay: true },
            );
          }
        } else if (
          Math.abs(playbackPosition.current - (video.playPosition ?? 0)) > 5000
        ) {
          video.playPosition = playbackPosition.current;
        }
      } else {
        setPlaybackStatus(null);
      }
    },
    [video, previousDuration, navigation, settings],
  );

  return (
    <SafeAreaView
      style={[styles.container, { backgroundColor: theme.colors.background }]}
    >
      <SchemeOverride scheme="dark" />
      <Video
        ref={videoRef}
        style={styles.video}
        resizeMode={ResizeMode.CONTAIN}
        onPlaybackStatusUpdate={onStatus}
      />
      {videoRef.current && playbackStatus && (
        <Overlay
          seek={seek}
          previousDuration={previousDuration.current}
          totalDuration={video.totalDuration}
          status={playbackStatus}
          video={videoRef.current}
        />
      )}
    </SafeAreaView>
  );
}
