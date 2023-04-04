import { StackRouter } from "@react-navigation/native";
import { Navigator, Slot, useRouter } from "expo-router";
import { useMemo, useRef } from "react";
import { DrawerLayoutAndroid, StyleSheet, View } from "react-native";
import { SafeAreaView } from "react-native-safe-area-context";
import {
  useLibraries,
  useLibrary,
  usePlaylist,
  usePlaylists,
} from "../modules/util";
import { LibraryState, LibraryType, PlaylistState } from "../modules/state";
import { Appbar, Drawer } from "react-native-paper";
import { MaterialIcons } from "@expo/vector-icons";

export default function LibraryNav() {
  let drawer = useRef<DrawerLayoutAndroid>(null);
  let openDrawer = useMemo(() => () => drawer.current?.openDrawer(), [drawer]);
  let closeDrawer = useMemo(
    () => () => drawer.current?.closeDrawer(),
    [drawer]
  );
  let renderDrawer = useMemo(
    () => () => <DrawerContent closeDrawer={closeDrawer} />,
    [closeDrawer]
  );

  return (
    <Navigator router={StackRouter}>
      <DrawerLayoutAndroid
        ref={drawer}
        renderNavigationView={renderDrawer}
        drawerWidth={300}
      >
        <Header openDrawer={openDrawer} />
        <Slot />
      </DrawerLayoutAndroid>
    </Navigator>
  );
}

function DrawerContent({ closeDrawer }: { closeDrawer: () => void }) {
  let libraries = useLibraries();
  let playlists = usePlaylists();

  let router = useRouter();

  let openLibrary = (library: LibraryState) => {
    router.push({
      pathname: "/media/library",
      params: {
        server: library.server.id,
        library: library.id,
      },
    });
    closeDrawer();
  };

  let openPlaylist = (playlist: PlaylistState) => {
    router.push({
      pathname: "/media/playlist",
      params: {
        server: playlist.server.id,
        playlist: playlist.id,
      },
    });
    closeDrawer();
  };

  return (
    <View style={styles.drawer}>
      <SafeAreaView edges={["top", "bottom", "left"]}>
        {libraries.length > 0 && (
          <Drawer.Section title="Libraries" showDivider={playlists.length > 0}>
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
          <Drawer.Section title="Playlists" showDivider={false}>
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
      </SafeAreaView>
    </View>
  );
}

function Header({ openDrawer }: { openDrawer: () => void }) {
  let playlist = usePlaylist();
  let library = useLibrary();
  let root = playlist ?? library;

  return (
    <Appbar.Header>
      <Appbar.Action icon="menu" onPress={openDrawer} />
      <Appbar.Content title={root.title} />
    </Appbar.Header>
  );
}

const styles = StyleSheet.create({
  drawer: {
    flex: 1,
  },
});
