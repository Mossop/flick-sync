import {
  StackActionHelpers,
  StackNavigationState,
  StackRouter,
  StackRouterOptions,
  createNavigatorFactory,
  useNavigationBuilder,
} from "@react-navigation/native";
import {
  NativeStackNavigationEventMap,
  NativeStackNavigationOptions,
  NativeStackView,
} from "@react-navigation/native-stack";
import { DrawerLayoutAndroid, View, StyleSheet } from "react-native";
import { SafeAreaView } from "react-native-safe-area-context";
import { NativeStackNavigatorProps } from "@react-navigation/native-stack/lib/typescript/src/types";
import { ReactNode, createContext, useContext, useMemo, useRef } from "react";
import { Drawer } from "react-native-paper";
import {
  ScreenProps,
  namedIcon,
  useLibraries,
  usePlaylists,
} from "../modules/util";
import { Library, MovieLibrary, Playlist } from "../state";

const styles = StyleSheet.create({
  drawer: {
    flex: 1,
  },
});

interface LibraryParams {
  server: string;
  library: string;
}

interface PlaylistParams {
  server: string;
  playlist: string;
}

interface CollectionParams {
  server: string;
  collection: string;
}

interface ShowParams {
  server: string;
  show: string;
}

interface VideoParams {
  server: string;
  video: string;
  restart?: boolean;
}

export interface AppRoutes {
  library: LibraryParams | undefined;
  playlist: PlaylistParams;
  collection: CollectionParams;
  show: ShowParams;
  video: VideoParams;
  [key: string]: object | undefined;
}

export type AppScreenProps<R extends keyof AppRoutes = keyof AppRoutes> =
  ScreenProps<AppRoutes, R>;

type Navigation = ReturnType<
  typeof useNavigationBuilder<
    StackNavigationState<AppRoutes>,
    StackRouterOptions,
    StackActionHelpers<AppRoutes>,
    NativeStackNavigationOptions,
    NativeStackNavigationEventMap
  >
>["navigation"];

interface AppDrawer {
  openDrawer: () => void;
  closeDrawer: () => void;
}

const DrawerContext = createContext<AppDrawer>({
  openDrawer: () => {},
  closeDrawer: () => {},
});
export const useAppDrawer = () => useContext(DrawerContext);

function DrawerContent({ navigation }: { navigation: Navigation }) {
  let { closeDrawer } = useAppDrawer();
  let libraries = useLibraries();
  let playlists = usePlaylists();

  let tvIcon = useMemo(() => namedIcon("tv"), []);
  let settingsIcon = useMemo(() => namedIcon("settings"), []);

  let openLibrary = (library: Library) => {
    navigation.navigate("library", {
      server: library.server.id,
      library: library.id,
      screen: "contents",
    });
    closeDrawer();
  };

  let openPlaylist = (playlist: Playlist) => {
    navigation.navigate("playlist", {
      server: playlist.server.id,
      playlist: playlist.id,
    });
    closeDrawer();
  };

  let openSettings = () => {
    navigation.navigate("settings");
    closeDrawer();
  };

  return (
    <View style={styles.drawer}>
      <SafeAreaView edges={["top", "bottom", "left"]}>
        {libraries.length > 0 && (
          <Drawer.Section title="Libraries">
            {libraries.map((library) => (
              <Drawer.Item
                key={library.id}
                onPress={() => openLibrary(library)}
                icon={library instanceof MovieLibrary ? "movie" : tvIcon}
                label={library.title}
              />
            ))}
          </Drawer.Section>
        )}

        {playlists.length > 0 && (
          <Drawer.Section title="Playlists">
            {playlists.map((playlist) => (
              <Drawer.Item
                key={playlist.id}
                onPress={() => openPlaylist(playlist)}
                icon="playlist-play"
                label={playlist.title}
              />
            ))}
          </Drawer.Section>
        )}

        <Drawer.Item
          onPress={openSettings}
          icon={settingsIcon}
          label="Settings"
        />
      </SafeAreaView>
    </View>
  );
}

function AppNavigatorView({
  navigation,
  children,
}: {
  navigation: Navigation;
  children: ReactNode;
}) {
  let drawer = useRef<DrawerLayoutAndroid>(null);
  let appDrawer = useMemo(
    () => ({
      openDrawer: () => drawer.current?.openDrawer(),
      closeDrawer: () => drawer.current?.closeDrawer(),
    }),
    [],
  );

  return (
    <DrawerContext.Provider value={appDrawer}>
      <DrawerLayoutAndroid
        ref={drawer}
        renderNavigationView={() => <DrawerContent navigation={navigation} />}
        drawerWidth={300}
      >
        {children}
      </DrawerLayoutAndroid>
    </DrawerContext.Provider>
  );
}

function AppNavigator({
  id,
  initialRouteName,
  children,
  screenListeners,
  screenOptions,
  ...rest
}: NativeStackNavigatorProps) {
  const { state, descriptors, navigation, NavigationContent } =
    useNavigationBuilder<
      StackNavigationState<AppRoutes>,
      StackRouterOptions,
      StackActionHelpers<AppRoutes>,
      NativeStackNavigationOptions,
      NativeStackNavigationEventMap
    >(StackRouter, {
      id,
      initialRouteName,
      children,
      screenListeners,
      screenOptions: {
        ...screenOptions,
        headerShown: false,
      },
    });

  return (
    <NavigationContent>
      <AppNavigatorView navigation={navigation}>
        <NativeStackView
          {...rest}
          state={state}
          navigation={navigation}
          descriptors={descriptors}
        />
      </AppNavigatorView>
    </NavigationContent>
  );
}

export default createNavigatorFactory<
  StackNavigationState<AppRoutes>,
  NativeStackNavigationOptions,
  NativeStackNavigationEventMap,
  typeof AppNavigator
>(AppNavigator);
