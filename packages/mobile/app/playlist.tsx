import AppView from "../components/AppView";
import { AppScreenProps } from "../components/AppNavigator";
import { List, ListControls } from "../components/List";
import { ContainerType } from "../state";
import { usePlaylist } from "../store";

export default function Playlist({ route }: AppScreenProps<"playlist">) {
  if (!route.params) {
    throw new Error("Missing params for playlist route");
  }

  let playlist = usePlaylist(route.params.server, route.params.playlist);

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
        inset={true}
      />
    </AppView>
  );
}
