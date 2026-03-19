import { StyleSheet, View } from "react-native";
import { VideoMetadata, VideoView, useVideoPlayer } from "expo-video";
import { useEventListener } from "expo";
import { SafeAreaView } from "react-native-safe-area-context";
import * as StatusBar from "expo-status-bar";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import * as NavigationBar from "expo-navigation-bar";
import { useNavigation } from "@react-navigation/native";
import * as ScreenOrientation from "expo-screen-orientation";
import { OrientationLock } from "expo-screen-orientation";
import { NativeStackNavigationProp } from "@react-navigation/native-stack";
import { AppRoutes, AppScreenProps } from "../components/AppNavigator";
import { SchemeOverride } from "../components/ThemeProvider";
import { defer } from "../modules/util";
import { reportError, useAction } from "../components/Store";
import { useMediaStore, useVideo } from "../mediastore";
import { useSsdp } from "../mediastore/SsdpService";
import { PlaybackState } from "../state/base";
import { Overlay, PlaybackStatus } from "../components/VideoOverlay";

export const PLAYBACK_UPDATE_INTERVAL = 5000;

const styles = StyleSheet.create({
  container: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
  },
  inner: {
    width: "100%",
    height: "100%",
  },
  video: {
    width: "100%",
    height: "100%",
  },
});

function needsPersist(
  oldState: PlaybackState,
  newState: PlaybackState,
): boolean {
  if (newState.state == "inprogress" && oldState.state == "inprogress") {
    return (
      Math.abs(newState.position - oldState.position) > PLAYBACK_UPDATE_INTERVAL
    );
  }

  return oldState.state != newState.state;
}

export default function VideoPlayer({ route }: AppScreenProps<"video">) {
  let navigation = useNavigation<NativeStackNavigationProp<AppRoutes>>();
  let mediaStore = useMediaStore();
  let dispatchSetError = useAction(reportError);
  let { suspend, resume } = useSsdp();

  let { server, queue, index } = route.params;

  let video = useVideo(server, queue[index]);

  let startPosition =
    video.playbackState.state == "played" || route.params.restart
      ? 0
      : video.playPosition;
  let startState: PlaybackState =
    video.playbackState.state == "played" || route.params.restart
      ? { state: "unplayed" }
      : video.playbackState;

  let currentState = useRef<PlaybackState>(startState);
  let lastUpdate = useRef<PlaybackState>(startState);

  let setPlayState = useCallback(
    (state: PlaybackState) => {
      defer(mediaStore.setPlaybackState(video, state));
      lastUpdate.current = state;
    },
    [mediaStore, video],
  );

  let [playbackStatus, setPlaybackStatus] = useState<PlaybackStatus>(() => ({
    position: startPosition,
    duration: video.totalDuration,
    isPlaying: false,
  }));

  let uri: string | undefined = mediaStore.videoUri(video);
  if (!uri) {
    dispatchSetError("Unexpected non-downloaded video");
    navigation.pop();
  }

  let metadata: VideoMetadata = {
    title: video.title,
    artwork: mediaStore.thumbnailUri(video),
  };

  let player = useVideoPlayer({ uri, metadata }, (p) => {
    console.log(
      `Initializing video playback of ${uri} at position ${startPosition}`,
    );

    p.timeUpdateEventInterval = 1;
    p.keepScreenOnWhilePlaying = true;
    p.showNowPlayingNotification = true;
    p.currentTime = startPosition / 1000;
    p.play();
  });

  let seek = useCallback(
    (position: number): void => {
      let actualPosition = Math.min(Math.max(position, 0), video.totalDuration);

      // eslint-disable-next-line react-hooks/immutability
      player.currentTime = actualPosition / 1000;
      player.play();
    },
    [video, player],
  );

  useEffect(() => {
    suspend();
    return () => {
      resume();
    };
  }, [suspend, resume]);

  useEffect(() => {
    console.log("mount");
    StatusBar.setStatusBarHidden(true, "fade");

    defer(async () => {
      await NavigationBar.setVisibilityAsync("hidden");
      await ScreenOrientation.lockAsync(OrientationLock.LANDSCAPE);
    });

    return () => {
      console.log("unmount");
      StatusBar.setStatusBarHidden(false, "fade");

      defer(async () => {
        await NavigationBar.setVisibilityAsync("visible");
        await ScreenOrientation.unlockAsync();
      });
    };
  }, []);

  useEffect(() => {
    return () => {
      setPlayState(currentState.current);
    };
  }, [setPlayState]);

  useEventListener(player, "timeUpdate", ({ currentTime }) => {
    let position = currentTime * 1000;
    currentState.current = { state: "inprogress", position };

    setPlaybackStatus((prev) => ({
      ...prev,
      position: position,
    }));

    if (needsPersist(currentState.current, lastUpdate.current)) {
      setPlayState(currentState.current);
    }
  });

  useEventListener(player, "playingChange", ({ isPlaying }) => {
    setPlaybackStatus((prev) => ({
      ...prev,
      isPlaying,
    }));

    setPlayState(currentState.current);
  });

  useEventListener(player, "playToEnd", () => {
    if (index + 1 >= queue.length) {
      navigation.pop();
    } else {
      currentState.current = { state: "played" };
      setPlayState(currentState.current);
      navigation.setParams({
        index: index + 1,
        restart: true,
      });
    }
  });

  let setPlaying = useCallback(
    (playing: boolean) => {
      if (playing) {
        player.play();
      } else {
        player.pause();
      }
    },
    [player],
  );

  let previous = useMemo(() => {
    if (index > 0) {
      return () => {
        navigation.setParams({
          index: index - 1,
        });
      };
    }

    return undefined;
  }, [navigation, index]);

  let next = useMemo(() => {
    if (index + 1 < queue.length) {
      return () => {
        navigation.setParams({
          index: index + 1,
          restart: true,
        });
      };
    }

    return undefined;
  }, [navigation, index, queue]);

  return (
    <SafeAreaView style={[styles.container, { backgroundColor: "black" }]}>
      <View style={styles.inner}>
        <SchemeOverride scheme="dark" />
        <VideoView
          player={player}
          style={styles.video}
          contentFit="contain"
          nativeControls={false}
        />
        <Overlay
          goPrevious={previous}
          goNext={next}
          seek={seek}
          status={playbackStatus}
          setPlaying={setPlaying}
          video={video}
        />
      </View>
    </SafeAreaView>
  );
}
