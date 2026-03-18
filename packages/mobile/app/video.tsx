import { StyleSheet, View } from "react-native";
import { VideoMetadata, VideoView, useVideoPlayer } from "expo-video";
import { useEventListener } from "expo";
import { SafeAreaView } from "react-native-safe-area-context";
import * as StatusBar from "expo-status-bar";
import { useCallback, useEffect, useMemo, useState } from "react";
import * as NavigationBar from "expo-navigation-bar";
import { useNavigation } from "@react-navigation/native";
import * as ScreenOrientation from "expo-screen-orientation";
import { OrientationLock } from "expo-screen-orientation";
import { NativeStackNavigationProp } from "@react-navigation/native-stack";
import { AppRoutes, AppScreenProps } from "../components/AppNavigator";
import { SchemeOverride } from "../components/ThemeProvider";
import { isDownloaded } from "../state";
import { defer } from "../modules/util";
import { reportError, useAction } from "../components/Store";
import { useMediaStore, useVideo, useResolveUri } from "../store";
import { PlaybackState } from "../state/base";
import { Overlay, PlaybackStatus } from "../components/VideoOverlay";

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

export default function VideoPlayer({ route }: AppScreenProps<"video">) {
  let navigation = useNavigation<NativeStackNavigationProp<AppRoutes>>();
  let mediaStore = useMediaStore();
  let dispatchSetError = useAction(reportError);
  let resolveUri = useResolveUri();

  let { server, queue, index } = route.params;

  let video = useVideo(server, queue[index]);

  let setPlayState = useCallback(
    (state: PlaybackState) => {
      defer(mediaStore.setPlaybackState(server, queue[index], state));
    },
    [mediaStore, server, queue, index],
  );
  let setPlayPosition = useCallback(
    (position: number) => {
      setPlayState({ state: "inprogress", position });
    },
    [setPlayState],
  );

  let { restart } = route.params;

  let [playbackStatus, setPlaybackStatus] = useState<PlaybackStatus>(() => ({
    position: restart ? 0 : video.playPosition,
    duration: video.totalDuration,
    isPlaying: false,
  }));

  let uri: string | undefined = undefined;
  if (isDownloaded(video.download)) {
    uri = resolveUri(video.download.path);
  } else {
    dispatchSetError("Unexpected non-downloaded video");
    navigation.pop();
  }

  let metadata: VideoMetadata = {
    title: video.title,
    artwork:
      video.thumbnail.state == "stored"
        ? resolveUri(video.thumbnail.path)
        : undefined,
  };

  let player = useVideoPlayer({ uri, metadata }, (p) => {
    let position =
      video.playbackState.state == "played" || restart ? 0 : video.playPosition;
    console.log(
      `Initializing video playback of ${uri} at position ${position}`,
    );

    p.timeUpdateEventInterval = 1;
    p.keepScreenOnWhilePlaying = true;
    p.showNowPlayingNotification = true;
    p.currentTime = position / 1000;
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

  useEventListener(player, "timeUpdate", ({ currentTime }) => {
    let currentPosition = currentTime * 1000;
    setPlaybackStatus((prev) => ({
      ...prev,
      position: currentPosition,
    }));

    if (Math.abs(currentPosition - video.playPosition) > 5000) {
      setPlayPosition(currentPosition);
    }
  });

  useEventListener(player, "playingChange", ({ isPlaying }) => {
    setPlaybackStatus((prev) => ({
      ...prev,
      isPlaying,
    }));
  });

  useEventListener(player, "playToEnd", () => {
    if (index + 1 >= queue.length) {
      navigation.pop();
    } else {
      setPlayState({ state: "played" });
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
