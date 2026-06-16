import { StyleSheet, View } from "react-native";
import {
  VideoSource,
  VideoView,
  createVideoPlayer,
  VideoPlayer as ExpoVideoPlayer,
} from "expo-video";
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
import {
  reportError,
  setViewSize,
  useAction,
  useSettings,
  ViewSize,
} from "../components/Store";
import { useMediaStore, useVideo } from "../mediastore";
import { useSsdpSuspense } from "../mediastore/SsdpService";
import { PlaybackState } from "../state/base";
import { Overlay, PlaybackStatus } from "../components/VideoOverlay";
import { Video } from "../state";

export const PLAYBACK_UPDATE_INTERVAL = 5000;

const styles = StyleSheet.create({
  container: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: "black",
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

function useVideoPlayer(
  videoSource: VideoSource,
  init: (player: ExpoVideoPlayer) => void,
): ExpoVideoPlayer | null {
  let currentPlayer = useRef<ExpoVideoPlayer | null>(null);
  let [player, setPlayer] = useState<ExpoVideoPlayer | null>(null);

  useEffect(() => {
    currentPlayer.current?.release();

    currentPlayer.current = createVideoPlayer(videoSource);
    init(currentPlayer.current);
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setPlayer(currentPlayer.current);

    return () => {
      currentPlayer.current?.release();
      currentPlayer.current = null;
    };
  }, [init, videoSource]);

  return player;
}

function VideoDisplay({
  uri,
  player,
  video,
  route,
}: {
  uri: string;
  player: ExpoVideoPlayer;
  video: Video;
  route: AppScreenProps<"video">["route"];
}) {
  let navigation = useNavigation<NativeStackNavigationProp<AppRoutes>>();
  let mediaStore = useMediaStore();
  let dispatchSetError = useAction(reportError);
  let dispatchSetViewSize = useAction(setViewSize);
  let viewSize = useSettings().viewSize ?? ViewSize.Safe;

  let { queue, index } = route.params;

  let [playbackStatus, setPlaybackStatus] = useState<PlaybackStatus>(() => ({
    position: player.currentTime * 1000,
    duration: video.totalDuration,
    isPlaying: false,
  }));

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

  useEffect(() => {
    return () => {
      setPlayState(currentState.current);
    };
  }, [setPlayState]);

  useEffect(() => {
    player.play();
  }, [player]);

  useEventListener(player, "timeUpdate", ({ currentTime }) => {
    if (player.status == "loading") {
      return;
    }

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

  useEventListener(player, "statusChange", ({ status, error }) => {
    console.log(`Video player status changed to ${status}`);

    if (status == "error") {
      let errorMessage = error?.message ?? "Unknown video playback error";
      dispatchSetError(`Failed to play ${video.title}: ${errorMessage}`);
      setPlaybackStatus((prev) => ({
        ...prev,
        isPlaying: false,
      }));
      return;
    }

    if (status == "idle" || status == "loading") {
      setPlaybackStatus((prev) => ({
        ...prev,
        isPlaying: false,
      }));
      return;
    }

    setPlaybackStatus((prev) => ({
      ...prev,
      position: player.currentTime * 1000,
      duration: video.totalDuration,
    }));
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

  let updateViewSize = useCallback(
    (viewSize: ViewSize) => {
      dispatchSetViewSize(viewSize);
    },
    [dispatchSetViewSize],
  );

  let seek = useCallback(
    (position: number): void => {
      let actualPosition = Math.min(Math.max(position, 0), video.totalDuration);

      player.seekBy(actualPosition / 1000 - player.currentTime);
      player.play();
    },
    [video, player],
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
    <>
      <VideoView
        key={uri}
        player={player}
        style={styles.video}
        contentFit="contain"
        nativeControls={false}
      />
      <Overlay
        setViewSize={updateViewSize}
        goPrevious={previous}
        goNext={next}
        seek={seek}
        status={playbackStatus}
        setPlaying={setPlaying}
        viewSize={viewSize}
        video={video}
      />
    </>
  );
}

export default function VideoPlayer({ route }: AppScreenProps<"video">) {
  let navigation = useNavigation<NativeStackNavigationProp<AppRoutes>>();
  let mediaStore = useMediaStore();
  let dispatchSetError = useAction(reportError);
  let viewSize = useSettings().viewSize ?? ViewSize.Safe;

  let { server, queue, index } = route.params;

  let video = useVideo(server, queue[index]);

  let startPosition = useMemo(
    () =>
      video.playbackState.state == "played" || route.params.restart
        ? 0
        : video.playPosition,
    [video, route.params.restart],
  );

  let uri: string | undefined = mediaStore.videoUri(video);
  if (!uri) {
    dispatchSetError("Unexpected non-downloaded video");
    navigation.pop();
  }

  let artwork = mediaStore.thumbnailUri(video);

  let playerInit = useCallback(
    (player: ExpoVideoPlayer) => {
      player.timeUpdateEventInterval = 1;
      player.keepScreenOnWhilePlaying = true;
      player.showNowPlayingNotification = true;
      player.currentTime = startPosition / 1000;
    },
    [startPosition],
  );

  let videoSource = useMemo(
    (): VideoSource => ({
      uri,
      metadata: {
        title: video.title,
        artwork,
      },
    }),
    [uri, video.title, artwork],
  );

  let player = useVideoPlayer(videoSource, playerInit);

  useSsdpSuspense();

  useEffect(() => {
    StatusBar.setStatusBarHidden(true, "fade");

    defer(async () => {
      await NavigationBar.setVisibilityAsync("hidden");
      await ScreenOrientation.lockAsync(OrientationLock.LANDSCAPE);
    });

    return () => {
      StatusBar.setStatusBarHidden(false, "fade");

      defer(async () => {
        await NavigationBar.setVisibilityAsync("visible");
        await ScreenOrientation.unlockAsync();
      });
    };
  }, []);

  return (
    <SafeAreaView
      edges={viewSize == ViewSize.Zoom ? [] : undefined}
      style={styles.container}
    >
      <View style={styles.inner}>
        <SchemeOverride scheme="dark" />
        {uri && player && (
          <VideoDisplay uri={uri} video={video} route={route} player={player} />
        )}
      </View>
    </SafeAreaView>
  );
}
