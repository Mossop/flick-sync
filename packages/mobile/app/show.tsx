import { useMemo } from "react";
import AppView from "../components/AppView";
import { useMediaState, byIndex } from "../modules/util";
import { AppScreenProps } from "../components/AppNavigator";
import { List, ListControls } from "../components/List";
import { ContainerType } from "../state";

export default function Show({ route }: AppScreenProps<"show">) {
  let mediaState = useMediaState();
  if (!route.params) {
    throw new Error("Missing params for playlist route");
  }

  let show = mediaState
    .getServer(route.params.server)
    .getShow(route.params.show);

  let episodes = useMemo(
    () => byIndex(show.seasons.flatMap((ss) => ss.episodes)),
    [show],
  );

  return (
    <AppView
      title={show.title}
      actions={<ListControls id={show.id} container={ContainerType.Show} />}
    >
      <List id={show.id} container={ContainerType.Show} items={episodes} />
    </AppView>
  );
}
