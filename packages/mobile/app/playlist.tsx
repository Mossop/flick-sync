import { Text } from "react-native";
import { ScreenProps, usePlaylists } from "../modules/util";
import AppView from "../components/AppView";

export default function Playlist({ route }: ScreenProps<"playlist">) {
  let playlists = usePlaylists();
  let currentPlaylist = playlists.find(
    (playlist) =>
      playlist.server.id == route.params.server &&
      playlist.id == route.params.playlist,
  );

  return (
    <AppView title={currentPlaylist?.title ?? ""}>
      <Text>{currentPlaylist?.title}</Text>
    </AppView>
  );
}
