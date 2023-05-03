import { ScrollView } from "react-native";
import AppView from "../components/AppView";
import { useMediaState } from "../components/AppState";
import { AppScreenProps } from "../components/AppNavigator";
import Videos from "../components/Videos";

export default function Playlist({ route }: AppScreenProps<"playlist">) {
  let mediaState = useMediaState();
  if (!route.params) {
    throw new Error("Missing params for playlist route");
  }

  let playlist = mediaState
    .getServer(route.params.server)
    .getPlaylist(route.params.playlist);
  if (!playlist) {
    throw new Error("Invalid params for playlist route");
  }

  return (
    <AppView title={playlist.title}>
      <ScrollView>
        <Videos videos={playlist.videos} />
      </ScrollView>
    </AppView>
  );
}
