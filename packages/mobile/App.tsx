import { NavigationContainer } from "@react-navigation/native";
import { AppStateProvider } from "./components/AppState";
import { Provider as PaperProvider } from "react-native-paper";
import { createNativeStackNavigator } from "@react-navigation/native-stack";
import Video from "./app/video";
import createAppNavigator from "./components/AppNavigator";
import Settings from "./app/settings";
import Playlist from "./app/playlist";
import { createMaterialBottomTabNavigator } from "@react-navigation/material-bottom-tabs";
import LibraryContent from "./app/contents";
import LibraryCollections from "./app/collections";
import { ScreenProps, useLibraries } from "./modules/util";
import { useMemo } from "react";

const LibraryNav = createMaterialBottomTabNavigator();

function Library({ route }: ScreenProps) {
  let libraries = useLibraries();
  let library = useMemo(() => {
    let params = route.params ?? {};
    return (
      libraries.find(
        (lib) =>
          // @ts-ignore
          lib.server.id == params["server"] &&
          // @ts-ignore
          lib.id.toString() == params["library"]
      ) ?? libraries[0]
    );
  }, [libraries, route.params]);

  return (
    <LibraryNav.Navigator initialRouteName="contents">
      <LibraryNav.Screen
        name="contents"
        component={() => <LibraryContent library={library} />}
      />
      <LibraryNav.Screen
        name="collections"
        component={() => <LibraryCollections library={library} />}
      />
    </LibraryNav.Navigator>
  );
}

const AppNav = createAppNavigator();

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
