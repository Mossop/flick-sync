import { createMaterialBottomTabNavigator } from "@react-navigation/material-bottom-tabs";
import { useMemo } from "react";
import { AppScreenProps } from "../components/AppNavigator";
import LibraryContent from "./libraryContents";
import LibraryCollections from "./libraryCollections";
import { useLibraries } from "../modules/util";

const LibraryNav = createMaterialBottomTabNavigator();

export default function Library({ route }: AppScreenProps<"library">) {
  let libraries = useLibraries();
  let library = useMemo(
    () =>
      libraries.find(
        (lib) =>
          lib.server.id == route.params?.server &&
          lib.id == route.params?.library,
      ) ?? libraries[0],
    [libraries, route.params],
  );

  if (!library) {
    return null;
  }

  if (library.collections().length > 0) {
    return (
      <LibraryNav.Navigator initialRouteName="contents">
        <LibraryNav.Screen
          name="contents"
          options={{
            tabBarIcon: "bookshelf",
            tabBarLabel: "Library",
          }}
        >
          {() => <LibraryContent library={library} />}
        </LibraryNav.Screen>
        <LibraryNav.Screen
          name="collections"
          options={{
            tabBarIcon: "bookmark-box-multiple",
            tabBarLabel: "Collections",
          }}
        >
          {() => <LibraryCollections library={library} />}
        </LibraryNav.Screen>
      </LibraryNav.Navigator>
    );
  }

  return <LibraryContent library={library} />;
}
