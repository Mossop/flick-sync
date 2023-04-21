import { StyleSheet } from "react-native";
import { Video, ResizeMode } from "expo-av";
import { SafeAreaView } from "react-native-safe-area-context";
import * as StatusBar from "expo-status-bar";
import { useEffect } from "react";
import * as NavigationBar from "expo-navigation-bar";
import { useAppState } from "../components/AppState";
import { AppScreenProps } from "../components/AppNavigator";
import { isDownloaded } from "../modules/state";

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
});

export default function VideoPlayer({ route }: AppScreenProps<"video">) {
  let appState = useAppState();

  if (!route.params) {
    throw new Error("Missing params for playlist route");
  }

  let video = appState.mediaState.servers
    .get(route.params.server)
    ?.videos.get(route.params.video);

  let part = video?.parts?.[route.params.part ?? 0];

  if (!part) {
    throw new Error("Incorrect params for video route");
  }

  let { download } = part;
  if (!isDownloaded(download)) {
    throw new Error("Unexpected missing download");
  }

  useEffect(() => {
    NavigationBar.setVisibilityAsync("hidden");
    StatusBar.setStatusBarHidden(true, "fade");

    return () => {
      StatusBar.setStatusBarHidden(false, "fade");
      NavigationBar.setVisibilityAsync("visible");
    };
  }, []);

  return (
    <SafeAreaView style={styles.container}>
      <Video
        style={styles.video}
        source={{
          uri: appState.path(download.path),
        }}
        useNativeControls
        shouldPlay
        resizeMode={ResizeMode.CONTAIN}
      />
    </SafeAreaView>
  );
}
