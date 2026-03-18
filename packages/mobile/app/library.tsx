import {
  BottomTabBarProps,
  createBottomTabNavigator,
} from "@react-navigation/bottom-tabs";
import { BottomNavigation } from "react-native-paper";
import { useMemo } from "react";
import { AppScreenProps } from "../components/AppNavigator";
import LibraryContent from "./libraryContents";
import LibraryCollections from "./libraryCollections";
import { useLibraries } from "../store";
import { MaterialCommunityIcons } from "@expo/vector-icons";
import { CommonActions } from "@react-navigation/native";

const LibraryNav = createBottomTabNavigator();

function NavigationBar({
  navigation,
  state,
  descriptors,
  insets,
}: BottomTabBarProps) {
  return (
    <BottomNavigation.Bar
      navigationState={state}
      safeAreaInsets={insets}
      onTabPress={({ route, ...pressEvent }) => {
        let event = navigation.emit({
          type: "tabPress",
          target: route.key,
          canPreventDefault: true,
        });

        if (event.defaultPrevented) {
          pressEvent.preventDefault();
        } else {
          navigation.dispatch({
            ...CommonActions.navigate(route.name, route.params),
            target: state.key,
          });
        }
      }}
      renderIcon={({ route, focused, color }) =>
        descriptors[route.key].options.tabBarIcon?.({
          focused,
          color,
          size: 24,
        }) ?? null
      }
      getLabelText={({ route }) => {
        let { options } = descriptors[route.key];
        let label =
          typeof options.tabBarLabel === "string"
            ? options.tabBarLabel
            : typeof options.title === "string"
              ? options.title
              : route.name;

        return label;
      }}
    />
  );
}

export default function Library({ route }: AppScreenProps<"library">) {
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
      <LibraryNav.Navigator
        screenOptions={{ animation: "shift", headerShown: false }}
        initialRouteName="contents"
        tabBar={NavigationBar}
      >
        <LibraryNav.Screen
          name="contents"
          options={{
            tabBarIcon: () => (
              <MaterialCommunityIcons name="bookshelf" size={22} />
            ),
            tabBarLabel: "Library",
          }}
        >
          {() => <LibraryContent library={library} />}
        </LibraryNav.Screen>
        <LibraryNav.Screen
          name="collections"
          options={{
            tabBarIcon: () => (
              <MaterialCommunityIcons name="bookmark-box-multiple" size={22} />
            ),
            tabBarLabel: "Collections",
          }}
        >
          {() => <LibraryCollections library={library} />}
        </LibraryNav.Screen>
      </LibraryNav.Navigator>
    );
  }

  return <LibraryContent library={library} />;
}
