import { useMemo } from "react";
import { ScrollView } from "react-native";
import AppView from "../components/AppView";
import { useMediaState } from "../components/AppState";
import { AppScreenProps } from "../components/AppNavigator";
import { byIndex } from "../modules/util";
import { List, ListControls, Type } from "../components/List";

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
      actions={<ListControls id={show.id} type={Type.Episode} />}
    >
      <ScrollView>
        <List id={show.id} type={Type.Episode} items={episodes} />
      </ScrollView>
    </AppView>
  );
}
