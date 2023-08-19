import registerRootComponent from "expo/build/launch/registerRootComponent";
import TrackPlayer from "react-native-track-player";
import service from "./modules/playback";

import App from "./App";

registerRootComponent(App);
TrackPlayer.registerPlaybackService(() => service);
