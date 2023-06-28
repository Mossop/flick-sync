import { NavigationContainer } from "@react-navigation/native";
import { createNativeStackNavigator } from "@react-navigation/native-stack";
import { createMaterialBottomTabNavigator } from "@react-navigation/material-bottom-tabs";
import { useEffect, useMemo } from "react";
import * as NavigationBar from "expo-navigation-bar";
import { GestureHandlerRootView } from "react-native-gesture-handler";
import { AppStateProvider } from "./components/AppState";
import Video from "./app/video";
import createAppNavigator, {
  AppScreenProps,
  AppRoutes,
} from "./components/AppNavigator";
import Settings from "./app/settings";
import Playlist from "./app/playlist";
import LibraryContent from "./app/libraryContents";
import LibraryCollections from "./app/libraryCollections";
import { useLibraries } from "./modules/util";
import Collection from "./app/collection";
import Show from "./app/show";
import { ThemeProvider } from "./components/ThemeProvider";

const LibraryNav = createMaterialBottomTabNavigator();

function Library({ route }: AppScreenProps<"library">) {
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

const AppNav = createAppNavigator<AppRoutes>();

function App() {
  return (
    <AppNav.Navigator initialRouteName="library">
      <AppNav.Screen name="library" component={Library} />
      <AppNav.Screen name="collection" component={Collection} />
      <AppNav.Screen name="show" component={Show} />
      <AppNav.Screen name="playlist" component={Playlist} />
      <AppNav.Screen name="settings" component={Settings} />
    </AppNav.Navigator>
  );
}

const RootStack = createNativeStackNavigator();

export default function Root() {
  useEffect(() => {
    NavigationBar.setBehaviorAsync("overlay-swipe");
  }, []);

  return (
    <GestureHandlerRootView style={{ flex: 1 }}>
      <AppStateProvider>
        <NavigationContainer>
          <ThemeProvider>
            <RootStack.Navigator
              initialRouteName="app"
              screenOptions={{ headerShown: false }}
            >
              <RootStack.Screen name="app" component={App} />
              {/* @ts-ignore */}
              <RootStack.Screen name="video" component={Video} />
            </RootStack.Navigator>
          </ThemeProvider>
        </NavigationContainer>
      </AppStateProvider>
    </GestureHandlerRootView>
  );
}
