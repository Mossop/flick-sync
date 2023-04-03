import { StackRouter } from "@react-navigation/native";
import { Link, Navigator, Slot, useRouter } from "expo-router";
import { useMemo, useRef } from "react";
import { DrawerLayoutAndroid, Pressable, View, StyleSheet } from "react-native";
import { SafeAreaView } from "react-native-safe-area-context";
import { MaterialIcons } from "@expo/vector-icons";
import { Divider, IconButton, Text } from "@react-native-material/core";
import * as Styles from "../modules/styles";
import {
  Library,
  Playlist,
  useLibraries,
  useLibrary,
  usePlaylists,
} from "../modules/util";

export default function LibraryNav() {
  let drawer = useRef<DrawerLayoutAndroid>(null);
  let openDrawer = useMemo(() => () => drawer.current?.openDrawer(), [drawer]);
  let closeDrawer = useMemo(
    () => () => drawer.current?.closeDrawer(),
    [drawer]
  );
  let renderDrawer = useMemo(
    () => () => <Drawer closeDrawer={closeDrawer} />,
    [closeDrawer]
  );

  return (
    <Navigator router={StackRouter}>
      <DrawerLayoutAndroid ref={drawer} renderNavigationView={renderDrawer}>
        <SafeAreaView mode="margin" edges={["top"]} style={{ height: 0 }} />
        <Header openDrawer={openDrawer} />
        <Slot />
      </DrawerLayoutAndroid>
    </Navigator>
  );
}

function Drawer({ closeDrawer }: { closeDrawer: () => void }) {
  let libraries = useLibraries();
  let playlists = usePlaylists();

  let router = useRouter();

  let openLibrary = (library: Library) => {
    router.push({
      pathname: "/media/library",
      params: {
        server: library.server,
        library: library.id,
      },
    });
    closeDrawer();
  };

  let openPlaylist = (playlist: Playlist) => {
    router.push({
      pathname: "/media/playlist",
      params: {
        server: playlist.server,
        playlist: playlist.id,
      },
    });
    closeDrawer();
  };

  return (
    <SafeAreaView edges={["top", "bottom", "left"]}>
      <View style={styles.drawer}>
        {libraries.map((library) => (
          <Pressable
            key={library.id}
            style={styles.drawerItem}
            onPress={() => openLibrary(library)}
          >
            <MaterialIcons
              key={library.id}
              name="video-library"
              size={Styles.HEADING_BUTTON_SIZE}
              style={Styles.buttonIcon}
            />
            <Text variant="h6">{library.title}</Text>
          </Pressable>
        ))}

        <Divider />

        {playlists.map((playlist) => (
          <Pressable
            key={playlist.id}
            style={styles.drawerItem}
            onPress={() => openPlaylist(playlist)}
          >
            <MaterialIcons
              key={playlist.id}
              name="playlist-play"
              size={Styles.HEADING_BUTTON_SIZE}
              style={Styles.buttonIcon}
            />
            <Text variant="h6">{playlist.title}</Text>
          </Pressable>
        ))}
      </View>
    </SafeAreaView>
  );
}

function Header({ openDrawer }: { openDrawer: () => void }) {
  let library = useLibrary();

  return (
    <View style={styles.header}>
      <IconButton
        onPress={openDrawer}
        icon={(props) => <MaterialIcons name="menu" {...props} />}
      />
      <Text variant="h6">{library.title}</Text>
    </View>
  );
}

const styles = StyleSheet.create({
  header: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "flex-start",
  },
  drawer: {
    flexDirection: "column",
    alignItems: "stretch",
    justifyContent: "flex-start",
  },
  drawerItem: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "flex-start",
  },
});
