import { Text } from "react-native";
import { ScreenProps, usePlaylists } from "../modules/util";
import AppView from "../components/AppView";

export default function Playlist({ route }: ScreenProps) {
  let playlists = usePlaylists();
  let params = route.params ?? {};
  let playlist = playlists.find(
    (playlist) =>
      // @ts-ignore
      playlist.server.id == params["server"] &&
      // @ts-ignore
      playlist.id.toString() == params["playlist"]
  );

  return (
    <AppView title={playlist?.title ?? ""}>
      <Text>{playlist?.title}</Text>
    </AppView>
  );
}
