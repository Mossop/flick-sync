import { NavigationContainer } from "@react-navigation/native";
import { createNativeStackNavigator } from "@react-navigation/native-stack";
import { GestureHandlerRootView } from "react-native-gesture-handler";
import { StyleSheet } from "react-native";
import { Suspense } from "react";
import * as SplashScreen from "expo-splash-screen";
import { StoreProvider, useSelector } from "./components/Store";
import Notification from "./components/Notification";
import MediaStorePicker from "./app/storepicker";
import Video from "./app/video";
import createAppNavigator from "./components/AppNavigator";
import Library from "./app/library";
import Settings from "./app/settings";
import Playlist from "./app/playlist";
import Collection from "./app/collection";
import Show from "./app/show";
import { ThemeProvider } from "./components/ThemeProvider";
import { ActivityIndicator } from "react-native-paper";

SplashScreen.preventAutoHideAsync().catch(console.error);

const AppNav = createAppNavigator();

const styles = StyleSheet.create({
  loading: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
  },
});

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

function MediaStoreLoader() {
  let mediaStore = useSelector((s) => s.mediaStore);

  if (!mediaStore) {
    return <MediaStorePicker />;
  }

  return (
    <>
      <RootStack.Navigator
        initialRouteName="app"
        screenOptions={{ headerShown: false }}
      >
        <RootStack.Screen name="app" component={App} />
        {/* @ts-expect-error */}
        <RootStack.Screen name="video" component={Video} />
      </RootStack.Navigator>
      <Notification />
    </>
  );
}

export default function Root() {
  return (
    <StoreProvider>
      <GestureHandlerRootView style={{ flex: 1 }}>
        <NavigationContainer>
          <ThemeProvider>
            <Suspense fallback={<ActivityIndicator style={styles.loading} />}>
              <MediaStoreLoader />
            </Suspense>
          </ThemeProvider>
        </NavigationContainer>
      </GestureHandlerRootView>
    </StoreProvider>
  );
}
