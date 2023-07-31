import AppView from "../components/AppView";
import { useMediaState } from "../modules/util";
import { AppScreenProps } from "../components/AppNavigator";
import { List, ListControls } from "../components/List";
import { ContainerType } from "../state";

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
    <AppView
      title={playlist.title}
      actions={
        <ListControls id={playlist.id} container={ContainerType.Playlist} />
      }
    >
      <List
        id={playlist.id}
        container={ContainerType.Playlist}
        items={playlist.videos}
      />
    </AppView>
  );
}
