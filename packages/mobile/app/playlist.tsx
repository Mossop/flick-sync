import { Text } from "react-native";
import AppView from "../components/AppView";
import { useMediaState } from "../components/AppState";
import { AppScreenProps } from "../components/AppNavigator";

export default function Playlist({ route }: AppScreenProps<"playlist">) {
  let mediaState = useMediaState();
  if (!route.params) {
    throw new Error("Missing params for playlist route");
  }

  let playlist = mediaState.servers
    .get(route.params.server)
    ?.playlists.get(route.params.playlist);
  if (!playlist) {
    throw new Error("Invalid params for playlist route");
  }

  return (
    <AppView title={playlist.title}>
      <Text>{playlist.title}</Text>
    </AppView>
  );
}
