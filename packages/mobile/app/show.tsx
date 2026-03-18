import { useMemo } from "react";
import AppView from "../components/AppView";
import { byIndex } from "../modules/util";
import { AppScreenProps } from "../components/AppNavigator";
import { List, ListControls } from "../components/List";
import { ContainerType } from "../state";
import { useShow } from "../mediastore";

export default function Show({ route }: AppScreenProps<"show">) {
  if (!route.params) {
    throw new Error("Missing params for show route");
  }

  let show = useShow(route.params.server, route.params.show);

  let episodes = useMemo(
    () => byIndex(show.seasons.flatMap((ss) => ss.episodes)),
    [show],
  );

  return (
    <AppView
      title={show.title}
      actions={<ListControls id={show.id} container={ContainerType.Show} />}
    >
      <List
        id={show.id}
        container={ContainerType.Show}
        items={episodes}
        inset={true}
      />
    </AppView>
  );
}
