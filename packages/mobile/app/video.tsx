import { Pressable, StyleSheet, View } from "react-native";
import {
  Video as VideoComponent,
  ResizeMode,
  AVPlaybackStatusSuccess,
  AVPlaybackStatus,
} from "expo-av";
import { SafeAreaView } from "react-native-safe-area-context";
import * as StatusBar from "expo-status-bar";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import * as NavigationBar from "expo-navigation-bar";
import Animated, { FadeIn, FadeOut } from "react-native-reanimated";
import { IconButton, Text } from "react-native-paper";
import { useNavigation } from "@react-navigation/native";
import { activateKeepAwakeAsync, deactivateKeepAwake } from "expo-keep-awake";
import * as ScreenOrientation from "expo-screen-orientation";
import { OrientationLock } from "expo-screen-orientation";
import { NativeStackNavigationProp } from "@react-navigation/native-stack";
import { AppRoutes, AppScreenProps } from "../components/AppNavigator";
import { PADDING } from "../modules/styles";
import Scrubber from "../components/Scrubber";
import { SchemeOverride } from "../components/ThemeProvider";
import { isDownloaded, isMovie, Video } from "../state";
import { pad, useMediaState } from "../modules/util";
import {
  reportError,
  setPlaybackState,
  useAction,
  useStoragePath,
} from "../components/Store";
import { PlaybackState } from "../state/base";

const OVERLAY_TIMEOUT = 10000;

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
    backgroundColor: "#00000050",
  },
  overlayMeta: {
    flex: 1,
    flexDirection: "column",
    alignItems: "center",
    paddingTop: PADDING,
  },
  buttons: {
    flex: 1,
    flexDirection: "row",
    alignItems: "flex-start",
    justifyContent: "space-between",
  },
  controls: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "center",
  },
});

function useOverlayState(): [boolean, (state?: boolean) => void] {
  let [visible, setVisible] = useState(true);
  let timeout = useRef<NodeJS.Timeout | null>(null);

  let initTimeout = (duration?: number) => {
    if (timeout.current) {
      clearTimeout(timeout.current);
    }

    timeout.current = setTimeout(() => {
      timeout.current = null;
      setVisible(false);
    }, duration ?? OVERLAY_TIMEOUT);
  };

  if (!timeout.current && visible) {
    initTimeout(OVERLAY_TIMEOUT / 2);
  }

  return [
    visible,
    (state?: boolean) => {
      let newState = state ?? !visible;
      if (!newState) {
        setVisible(false);
        if (timeout.current) {
          clearTimeout(timeout.current);
          timeout.current = null;
        }
      } else {
        initTimeout();
        setVisible(true);
      }
    },
  ];
}

function OverlayMeta({ video }: { video: Video }) {
  if (isMovie(video)) {
    return (
      <View style={styles.overlayMeta}>
        <Text variant="titleLarge" numberOfLines={1} ellipsizeMode="tail">
          {video.title}
        </Text>
      </View>
    );
  }

  return (
    <View style={styles.overlayMeta}>
      <Text variant="titleLarge" numberOfLines={1} ellipsizeMode="tail">
        {video.season.show.title}
      </Text>
      <Text variant="titleMedium" numberOfLines={1} ellipsizeMode="tail">
        s{pad(video.season.index)}e{pad(video.index)} - {video.title}
      </Text>
    </View>
  );
}

function Overlay({
  seek,
  video,
  videoComponent,
  status,
  previousDuration,
  totalDuration,
  goPrevious,
  goNext,
}: {
  seek: (position: number) => Promise<void>;
  video: Video;
  videoComponent: VideoComponent;
  status: AVPlaybackStatusSuccess;
  previousDuration: number;
  totalDuration: number;
  goPrevious?: () => void;
  goNext?: () => void;
}) {
  let navigation = useNavigation<NativeStackNavigationProp<AppRoutes>>();
  let [visible, updateState] = useOverlayState();
  let position = previousDuration + status.positionMillis;
  let keepAlive = useCallback(() => updateState(true), [updateState]);

  let togglePlayback = useCallback(() => {
    videoComponent.setStatusAsync({ shouldPlay: !status.isPlaying });
    keepAlive();
  }, [videoComponent, status, keepAlive]);

  let skip = useCallback(
    (delta: number) => {
      seek(position + delta);
      keepAlive();
    },
    [seek, position, keepAlive],
  );

  let restart = useCallback(() => {
    seek(0);
    keepAlive();
  }, [seek, keepAlive]);

  let goBack = useCallback(() => {
    navigation.pop();
  }, [navigation]);

  let inQueue = goPrevious || goNext;

  let previous = useCallback(() => {
    if (goPrevious) {
      goPrevious();
      updateState(true);
    }
  }, [goPrevious, updateState]);

  let next = useCallback(() => {
    if (goNext) {
      goNext();
      updateState(true);
    }
  }, [goNext, updateState]);

  return (
    <Pressable style={styles.overlayContainer} onPress={() => updateState()}>
      {visible && (
        <Animated.View
          style={styles.overlay}
          entering={FadeIn}
          exiting={FadeOut}
        >
          <View style={styles.buttons}>
            <IconButton icon="replay" onPress={restart} size={40} />
            <OverlayMeta video={video} />
            <IconButton icon="close" onPress={goBack} size={40} />
          </View>
          <View style={styles.controls}>
            {inQueue && (
              <IconButton
                icon="skip-previous"
                disabled={goPrevious === undefined}
                onPress={previous}
                size={40}
              />
            )}
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
            {inQueue && (
              <IconButton
                icon="skip-next"
                disabled={goNext === undefined}
                onPress={next}
                size={40}
              />
            )}
          </View>
          <Scrubber
            position={position}
            totalDuration={totalDuration}
            onScrubbing={keepAlive}
            onScrubbingComplete={seek}
          />
        </Animated.View>
      )}
    </Pressable>
  );
}

export default function VideoPlayer({ route }: AppScreenProps<"video">) {
  let navigation = useNavigation<NativeStackNavigationProp<AppRoutes>>();
  let mediaState = useMediaState();
  let videoRef = useRef<VideoComponent | null>(null);
  let [playbackStatus, setPlaybackStatus] =
    useState<AVPlaybackStatusSuccess | null>(null);
  let initialized = useRef<string | null>(null);
  let dispatchSetError = useAction(reportError);
  let storagePath = useStoragePath();

  let { server, queue, index } = route.params;

  let dispatchSetPlaybackState = useAction(setPlaybackState);
  let setPlayState = useCallback(
    (state: PlaybackState) => {
      dispatchSetPlaybackState([server, queue[index]!, state]);
    },
    [dispatchSetPlaybackState, server, queue, index],
  );
  let setPlayPosition = useCallback(
    (position: number) => {
      setPlayState({ state: "inprogress", position });
    },
    [setPlayState],
  );

  let video = mediaState.getServer(server).getVideo(queue[index]!);
  let { restart } = route.params;

  let finalState = useRef(video.playbackState);

  let previousDuration = useRef<number>(0);
  let currentPart = useRef<number>();
  let seek = useCallback(
    async (position: number): Promise<void> => {
      previousDuration.current = 0;
      let targetPart = 0;
      let partPosition = Math.min(Math.max(position, 0), video.totalDuration);

      while (
        targetPart < video.parts.length - 1 &&
        video.parts[targetPart]!.duration >= partPosition
      ) {
        partPosition -= video.parts[targetPart]!.duration;
        previousDuration.current += video.parts[targetPart]!.duration;
        targetPart++;
      }

      if (targetPart === currentPart.current) {
        await videoRef.current!.playFromPositionAsync(partPosition);
      } else {
        try {
          await videoRef.current!.unloadAsync();
        } catch (e) {
          console.error(e);
        }

        currentPart.current = targetPart;
        let { download } = video.parts[targetPart]!;

        if (!isDownloaded(download)) {
          dispatchSetError("Unexpected non-downloaded part");
          navigation.pop();
          return;
        }

        console.log(`Loading ${download.path} at position ${partPosition}`);
        await videoRef.current!.loadAsync(
          { uri: storagePath(download.path) },
          {
            positionMillis: partPosition,
            shouldPlay: true,
            androidImplementation: "MediaPlayer",
          },
        );
      }
    },
    [video, dispatchSetError, storagePath, navigation],
  );

  useEffect(() => {
    NavigationBar.setVisibilityAsync("hidden");
    StatusBar.setStatusBarHidden(true, "fade");
    ScreenOrientation.lockAsync(OrientationLock.LANDSCAPE);
    console.log("mount");

    return () => {
      console.log("unmount");
      StatusBar.setStatusBarHidden(false, "fade");
      NavigationBar.setVisibilityAsync("visible");
      ScreenOrientation.unlockAsync();
      // eslint-disable-next-line react-hooks/exhaustive-deps
      videoRef.current?.unloadAsync();
      setPlaybackStatus(null);
    };
  }, []);

  useEffect(
    () => () => {
      setPlayState(finalState.current);
    },
    [setPlayState],
  );

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
    if (initialized.current !== video.id) {
      console.log("Initializing for new video");
      currentPart.current = undefined;
      seek(
        video.playbackState.state == "played" || restart
          ? 0
          : video.playPosition,
      );
      initialized.current = video.id;
    }
  }, [video, seek, restart]);

  let onStatus = useCallback(
    (avStatus: AVPlaybackStatus) => {
      if ("uri" in avStatus) {
        setPlaybackStatus(avStatus);
        let currentPosition =
          previousDuration.current + avStatus.positionMillis;

        if (currentPosition < 30000) {
          finalState.current = { state: "unplayed" };
        } else if (currentPosition > 0.95 * video.totalDuration) {
          finalState.current = { state: "played" };
        } else {
          finalState.current = {
            state: "inprogress",
            position: currentPosition,
          };
        }

        if (avStatus.didJustFinish) {
          if (currentPart.current == video.parts.length - 1) {
            finalState.current = { state: "played" };
            setPlayState(finalState.current);
            if (index + 1 >= queue.length) {
              navigation.pop();
            } else {
              navigation.setParams({
                index: index + 1,
                restart: true,
              });
            }
          } else {
            previousDuration.current +=
              video.parts[currentPart.current!]!.duration;
            currentPart.current = currentPart.current! + 1;
            setPlayPosition(previousDuration.current);

            let { download } = video.parts[currentPart.current!]!;
            if (!isDownloaded(download)) {
              dispatchSetError("Unexpected non-downloaded part");
              navigation.pop();
              return;
            }

            videoRef.current?.loadAsync(
              { uri: storagePath(download.path) },
              { positionMillis: 0, shouldPlay: true },
            );
          }
        } else if (Math.abs(currentPosition - video.playPosition) > 5000) {
          setPlayPosition(currentPosition);
        }
      } else {
        setPlaybackStatus(null);
      }
    },
    [
      video,
      previousDuration,
      navigation,
      dispatchSetError,
      storagePath,
      index,
      queue,
      setPlayState,
      setPlayPosition,
    ],
  );

  let onError = useCallback(
    (message: string) => {
      console.error(message);
      dispatchSetError("Video playback failed");
      navigation.pop();
    },
    [dispatchSetError, navigation],
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
      <SchemeOverride scheme="dark" />
      <VideoComponent
        ref={videoRef}
        style={styles.video}
        resizeMode={ResizeMode.CONTAIN}
        onPlaybackStatusUpdate={onStatus}
        onError={onError}
      />
      {videoRef.current && playbackStatus && (
        <Overlay
          goPrevious={previous}
          goNext={next}
          seek={seek}
          previousDuration={previousDuration.current}
          totalDuration={video.totalDuration}
          status={playbackStatus}
          videoComponent={videoRef.current}
          video={video}
        />
      )}
    </SafeAreaView>
  );
}
