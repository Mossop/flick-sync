import { SafeAreaView } from "react-native-safe-area-context";
import { Drawer } from "react-native-paper";
import { ActivityIndicator, StyleSheet } from "react-native";
import {
  createContext,
  useContext,
  ReactNode,
  useMemo,
  useState,
  Suspense,
} from "react";
import { Drawer as DrawerLayout } from "react-native-drawer-layout";
import { namedIcon } from "../modules/util";
import { useLibraries, usePlaylists } from "../mediastore";
import { Library, MovieLibrary, Playlist } from "../state";
import { AppNavigation } from "./AppNavigator";

const styles = StyleSheet.create({
  drawer: {
    flex: 1,
  },
});

interface AppDrawer {
  openDrawer: () => void;
  closeDrawer: () => void;
}

const DrawerContext = createContext<AppDrawer>({
  openDrawer: () => {},
  closeDrawer: () => {},
});

export const useAppDrawer = () => useContext(DrawerContext);

function DrawerContent({ navigation }: { navigation: AppNavigation }) {
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
    <SafeAreaView edges={["top", "bottom", "left"]} style={styles.drawer}>
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
  );
}

export default function DrawerView({
  navigation,
  children,
}: {
  navigation: AppNavigation;
  children: ReactNode;
}) {
  let [open, setOpen] = useState(false);

  let appDrawer = useMemo(
    () => ({
      openDrawer: () => setOpen(true),
      closeDrawer: () => setOpen(false),
    }),
    [],
  );

  return (
    <DrawerContext.Provider value={appDrawer}>
      <DrawerLayout
        open={open}
        onOpen={() => setOpen(true)}
        onClose={() => setOpen(false)}
        renderDrawerContent={() => (
          <Suspense fallback={<ActivityIndicator style={styles.drawer} />}>
            <DrawerContent navigation={navigation} />
          </Suspense>
        )}
      >
        {children}
      </DrawerLayout>
    </DrawerContext.Provider>
  );
}
