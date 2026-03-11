import {
  DefaultNavigatorOptions,
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
} from "@react-navigation/native-stack";
import { ScreenProps } from "../modules/util";
import DrawerView from "./Drawer";

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

export interface VideoParams {
  server: string;
  queue: string[];
  index: number;
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

export type AppNavigation = ReturnType<
  typeof useNavigationBuilder<
    StackNavigationState<AppRoutes>,
    StackRouterOptions,
    StackActionHelpers<AppRoutes>,
    NativeStackNavigationOptions,
    NativeStackNavigationEventMap
  >
>["navigation"];

type NativeStackNavigatorProps = DefaultNavigatorOptions<
  ParamListBase,
  undefined,
  StackNavigationState<ParamListBase>,
  NativeStackNavigationOptions,
  NativeStackNavigationEventMap,
  AppNavigation
>;

function AppNavigator(props: NativeStackNavigatorProps) {
  let { state, descriptors, navigation, NavigationContent } =
    useNavigationBuilder<
      StackNavigationState<AppRoutes>,
      StackRouterOptions,
      StackActionHelpers<AppRoutes>,
      NativeStackNavigationOptions,
      NativeStackNavigationEventMap
    >(StackRouter, props);

  let focusedRoute = state.routes[state.index];
  let descriptor = descriptors[focusedRoute.key];

  return (
    <NavigationContent>
      <DrawerView navigation={navigation}>{descriptor.render()}</DrawerView>
    </NavigationContent>
  );
}

export default createNavigatorFactory(AppNavigator);
