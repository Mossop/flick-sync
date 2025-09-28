import { DependencyList, useCallback, useRef, useState } from "react";
import { View, StyleSheet, Pressable } from "react-native";
import { IconButton, Text } from "react-native-paper";
import { NativeStackNavigationProp } from "@react-navigation/native-stack";
import { useNavigation } from "@react-navigation/native";
import Animated, { FadeIn, FadeOut } from "react-native-reanimated";
import { Video, isMovie } from "../state";
import Scrubber from "./Scrubber";
import { PADDING } from "../modules/styles";
import { pad } from "../modules/util";
import { AppRoutes } from "./AppNavigator";

export interface PlaybackStatus {
  position: number;
  duration: number;
  isPlaying: boolean;
}

const OVERLAY_TIMEOUT = 10000;

const styles = StyleSheet.create({
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

function usePrefixedCallbackBuilder(
  prefix: () => void,
): <T extends Function>(cb: T, deps: DependencyList) => T {
  return (cb, deps) =>
    // eslint-disable-next-line react-hooks/rules-of-hooks
    useCallback(
      // @ts-ignore
      (...args) => {
        prefix();
        return cb(...args);
      },
      // eslint-disable-next-line react-hooks/exhaustive-deps
      [prefix, cb, ...deps],
    );
}

function useOverlayState(): [
  visible: boolean,
  toggle: () => void,
  useOverlayAction: <T extends Function>(cb: T, deps: DependencyList) => T,
] {
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
    initTimeout(OVERLAY_TIMEOUT);
  }

  let updateState = useCallback((state?: boolean) => {
    setVisible((isVisible) => {
      let newState = state ?? !isVisible;
      if (!newState) {
        if (timeout.current) {
          clearTimeout(timeout.current);
          timeout.current = null;
        }
        return false;
      }

      initTimeout();
      return true;
    });
  }, []);

  let keepAlive = useCallback(() => updateState(true), [updateState]);
  let useOverlayAction = usePrefixedCallbackBuilder(keepAlive);

  return [visible, updateState, useOverlayAction];
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

export function Overlay({
  seek,
  video,
  setPlaying,
  status,
  goPrevious,
  goNext,
}: {
  seek: (position: number) => Promise<void>;
  video: Video;
  setPlaying: (playing: boolean) => void;
  status: PlaybackStatus;
  goPrevious?: () => void;
  goNext?: () => void;
}) {
  let navigation = useNavigation<NativeStackNavigationProp<AppRoutes>>();
  let [visible, updateState, useOverlayAction] = useOverlayState();

  let togglePlayback = useOverlayAction(() => {
    setPlaying(!status.isPlaying);
  }, [setPlaying, status]);

  let skip = useOverlayAction(
    (delta: number) => {
      seek(status.position + delta);
    },
    [seek, status],
  );

  let restart = useOverlayAction(() => {
    seek(0);
  }, [seek]);

  let goBack = useCallback(() => {
    navigation.pop();
  }, [navigation]);

  let inQueue = goPrevious || goNext;

  let previous = useOverlayAction(() => {
    if (goPrevious) {
      goPrevious();
    }
  }, [goPrevious]);

  let next = useOverlayAction(() => {
    if (goNext) {
      goNext();
    }
  }, [goNext]);

  let onScrub = useOverlayAction(() => {}, []);

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
              onPress={() => skip(-10000)}
              size={40}
            />
            <IconButton
              icon={status.isPlaying ? "pause" : "play"}
              onPress={togglePlayback}
              size={80}
            />
            <IconButton
              icon="fast-forward-10"
              onPress={() => skip(10000)}
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
            position={status.position}
            totalDuration={status.duration}
            onScrubbing={onScrub}
            onScrubbingComplete={seek}
          />
        </Animated.View>
      )}
    </Pressable>
  );
}
