import {
  ParamListBase,
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
import { ReactNode, createContext, useContext, useRef } from "react";
import { Drawer } from "react-native-paper";
import { MaterialIcons } from "@expo/vector-icons";
import { useLibraries, usePlaylists } from "../modules/util";
import { LibraryState, LibraryType, PlaylistState } from "../modules/state";

const styles = StyleSheet.create({
  drawer: {
    flex: 1,
  },
});

type Navigation = ReturnType<
  typeof useNavigationBuilder<
    StackNavigationState<ParamListBase>,
    StackRouterOptions,
    StackActionHelpers<ParamListBase>,
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

  let openLibrary = (library: LibraryState) => {
    navigation.navigate("library", {
      server: library.server.id,
      library: library.id.toString(),
      screen: "contents",
    });
    closeDrawer();
  };

  let openPlaylist = (playlist: PlaylistState) => {
    navigation.navigate("playlist", {
      server: playlist.server.id,
      playlist: playlist.id.toString(),
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
                icon={
                  library.type == LibraryType.Movie
                    ? "movie"
                    : (props) => <MaterialIcons name="tv" {...props} />
                }
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
          icon={(props) => <MaterialIcons name="settings" {...props} />}
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
  let appDrawer = {
    openDrawer: () => drawer.current?.openDrawer(),
    closeDrawer: () => drawer.current?.closeDrawer(),
  };

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
      StackNavigationState<ParamListBase>,
      StackRouterOptions,
      StackActionHelpers<ParamListBase>,
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
  StackNavigationState<ParamListBase>,
  NativeStackNavigationOptions,
  NativeStackNavigationEventMap,
  typeof AppNavigator
>(AppNavigator);
