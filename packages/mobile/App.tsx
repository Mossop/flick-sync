import { NavigationContainer } from "@react-navigation/native";
import { Provider as PaperProvider } from "react-native-paper";
import { createNativeStackNavigator } from "@react-navigation/native-stack";
import { createMaterialBottomTabNavigator } from "@react-navigation/material-bottom-tabs";
import { useMemo } from "react";
import { AppStateProvider } from "./components/AppState";
import Video from "./app/video";
import createAppNavigator from "./components/AppNavigator";
import Settings from "./app/settings";
import Playlist from "./app/playlist";
import LibraryContent from "./app/contents";
import LibraryCollections from "./app/collections";
import { Routes, ScreenProps, useLibraries } from "./modules/util";

const LibraryNav = createMaterialBottomTabNavigator();

function Library({ route }: ScreenProps<"library">) {
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

  if (library.collections.length > 0) {
    return (
      <LibraryNav.Navigator initialRouteName="contents">
        <LibraryNav.Screen
          name="contents"
          options={{
            tabBarIcon: "bookshelf",
            tabBarLabel: "Library",
          }}
        >
          {() => <LibraryContent library={library!} />}
        </LibraryNav.Screen>
        <LibraryNav.Screen
          name="collections"
          options={{
            tabBarIcon: "bookmark-box-multiple",
            tabBarLabel: "Collections",
          }}
        >
          {() => <LibraryCollections library={library!} />}
        </LibraryNav.Screen>
      </LibraryNav.Navigator>
    );
  }

  return <LibraryContent library={library} />;
}

const AppNav = createAppNavigator<Routes>();

function App() {
  return (
    <AppNav.Navigator initialRouteName="library">
      <AppNav.Screen name="library" component={Library} />
      <AppNav.Screen name="playlist" component={Playlist} />
      <AppNav.Screen name="settings" component={Settings} />
    </AppNav.Navigator>
  );
}

const RootStack = createNativeStackNavigator();

export default function Root() {
  return (
    <AppStateProvider>
      <NavigationContainer>
        <PaperProvider>
          <RootStack.Navigator
            initialRouteName="app"
            screenOptions={{ headerShown: false }}
          >
            <RootStack.Screen name="app" component={App} />
            <RootStack.Screen name="video" component={Video} />
          </RootStack.Navigator>
        </PaperProvider>
      </NavigationContainer>
    </AppStateProvider>
  );
}
