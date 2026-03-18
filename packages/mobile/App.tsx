import { NavigationContainer } from "@react-navigation/native";
import { createNativeStackNavigator } from "@react-navigation/native-stack";
import { GestureHandlerRootView } from "react-native-gesture-handler";
import { ActivityIndicator, StyleSheet } from "react-native";
import { Suspense } from "react";
import { StoreProvider } from "./components/Store";
import { MediaStoreProvider } from "./store";
import Notification from "./components/Notification";
import Video from "./app/video";
import createAppNavigator from "./components/AppNavigator";
import Library from "./app/library";
import Settings from "./app/settings";
import Playlist from "./app/playlist";
import Collection from "./app/collection";
import Show from "./app/show";
import { ThemeProvider } from "./components/ThemeProvider";

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

export default function Root() {
  return (
    <GestureHandlerRootView style={{ flex: 1 }}>
      <StoreProvider>
        <MediaStoreProvider>
          <NavigationContainer>
            <ThemeProvider>
              <Suspense fallback={<ActivityIndicator style={styles.loading} />}>
                <RootStack.Navigator
                  initialRouteName="app"
                  screenOptions={{ headerShown: false }}
                >
                  <RootStack.Screen name="app" component={App} />
                  {/* @ts-expect-error */}
                  <RootStack.Screen name="video" component={Video} />
                </RootStack.Navigator>
              </Suspense>
              <Notification />
            </ThemeProvider>
          </NavigationContainer>
        </MediaStoreProvider>
      </StoreProvider>
    </GestureHandlerRootView>
  );
}
