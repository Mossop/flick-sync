import { StyleSheet } from "react-native";
import { Video as VideoComponent, ResizeMode, AVPlaybackStatus } from "expo-av";
import { SafeAreaView } from "react-native-safe-area-context";
import * as StatusBar from "expo-status-bar";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import * as NavigationBar from "expo-navigation-bar";
import { useNavigation } from "@react-navigation/native";
import { activateKeepAwakeAsync, deactivateKeepAwake } from "expo-keep-awake";
import * as ScreenOrientation from "expo-screen-orientation";
import { OrientationLock } from "expo-screen-orientation";
import { NativeStackNavigationProp } from "@react-navigation/native-stack";
import { Event, useTrackPlayerEvents } from "react-native-track-player";
import { AppRoutes, AppScreenProps } from "../components/AppNavigator";
import { SchemeOverride } from "../components/ThemeProvider";
import { isDownloaded } from "../state";
import { useMediaState } from "../modules/util";
import {
  reportError,
  setPlaybackState,
  useAction,
  useStoragePath,
} from "../components/Store";
import { PlaybackState } from "../state/base";
import { Overlay, PlaybackStatus } from "../components/VideoOverlay";

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
});

export default function VideoPlayer({ route }: AppScreenProps<"video">) {
  let navigation = useNavigation<NativeStackNavigationProp<AppRoutes>>();
  let mediaState = useMediaState();
  let videoRef = useRef<VideoComponent | null>(null);
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

  let [playbackStatus, setPlaybackStatus] = useState<PlaybackStatus>(() => ({
    position: restart ? 0 : video.playPosition,
    duration: video.totalDuration,
    isPlaying: false,
  }));

  let finalState = useRef(video.playbackState);

  let seek = useCallback(
    async (position: number, isNewVideo: boolean = false): Promise<void> => {
      let actualPosition = Math.min(Math.max(position, 0), video.totalDuration);

      if (!isNewVideo) {
        await videoRef.current!.playFromPositionAsync(actualPosition);
      } else {
        try {
          await videoRef.current!.unloadAsync();
        } catch (e) {
          console.error(e);
        }

        let { download } = video;

        if (!isDownloaded(download)) {
          dispatchSetError("Unexpected non-downloaded video");
          navigation.pop();
          return;
        }

        console.log(`Loading ${download.path} at position ${actualPosition}`);
        await videoRef.current!.loadAsync(
          { uri: storagePath(download.path) },
          {
            positionMillis: actualPosition,
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
      seek(
        video.playbackState.state == "played" || restart
          ? 0
          : video.playPosition,
        true,
      );
      initialized.current = video.id;
    }
  }, [video, seek, restart]);

  let onStatus = useCallback(
    (avStatus: AVPlaybackStatus) => {
      if ("uri" in avStatus) {
        let currentPosition = avStatus.positionMillis;
        setPlaybackStatus({
          position: currentPosition,
          duration: video.totalDuration,
          isPlaying: avStatus.isPlaying,
        });

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
          finalState.current = { state: "played" };
          if (index + 1 >= queue.length) {
            navigation.pop();
          } else {
            setPlayState(finalState.current);
            navigation.setParams({
              index: index + 1,
              restart: true,
            });
          }
        } else if (Math.abs(currentPosition - video.playPosition) > 5000) {
          setPlayPosition(currentPosition);
        }
      }
    },
    [video, navigation, index, queue, setPlayState, setPlayPosition],
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

  useTrackPlayerEvents([Event.RemotePlay, Event.RemotePause], async () => {
    let videoComponent = videoRef.current;
    if (!videoComponent) {
      return;
    }

    let avStatus = await videoComponent.getStatusAsync();
    if ("uri" in avStatus) {
      await videoComponent.setStatusAsync({ shouldPlay: !avStatus.isPlaying });
    }
  });

  let setPlaying = useCallback(
    (playing: boolean) => {
      videoRef.current?.setStatusAsync({ shouldPlay: playing });
    },
    [videoRef],
  );

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
      <Overlay
        goPrevious={previous}
        goNext={next}
        seek={seek}
        status={playbackStatus}
        setPlaying={setPlaying}
        video={video}
      />
    </SafeAreaView>
  );
}
