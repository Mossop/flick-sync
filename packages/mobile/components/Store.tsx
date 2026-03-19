import {
  Middleware,
  configureStore,
  createAction,
  createReducer,
} from "@reduxjs/toolkit";
import {
  Provider,
  useDispatch,
  useSelector as useReduxState,
} from "react-redux";
import * as SplashScreen from "expo-splash-screen";
import AsyncStorage from "@react-native-async-storage/async-storage";
import { ReactNode, use, useCallback } from "react";
import { ContainerType } from "../state";
import { MediaStore } from "../mediastore/MediaStore";

const STORE_KEY = "store";
const SETTINGS_KEY = "settings#";

export enum Display {
  Grid = "grid",
  List = "list",
}

export enum Ordering {
  Index = "index",
  Title = "title",
  Duration = "duration",
  AirDate = "airdate",
}

export interface ListSetting {
  display: Display;
  ordering: Ordering;
}

export interface SettingsState {
  listSettings: Record<string, ListSetting>;
}

export interface StoreState {
  mediaStore: MediaStore | null;
  settings: SettingsState;
  notificationMessage?: string;
  discoveredServers: string[];
}

export function useSelector<Selected = unknown>(
  selector: (state: StoreState) => Selected,
) {
  return useReduxState<StoreState, Selected>(selector);
}

export function useAction<P>(
  action: (payload: P) => { type: string },
): (payload: P) => void {
  let dispatch = useDispatch();
  return useCallback(
    (payload: P) => dispatch(action(payload)),
    [dispatch, action],
  );
}

function defaultSetting(container: ContainerType): ListSetting {
  switch (container) {
    case ContainerType.Show:
      return {
        display: Display.List,
        ordering: Ordering.Index,
      };
    case ContainerType.Playlist:
      return {
        display: Display.List,
        ordering: Ordering.Index,
      };
    case ContainerType.MovieCollection:
      return {
        display: Display.Grid,
        ordering: Ordering.Index,
      };
    case ContainerType.ShowCollection:
      return {
        display: Display.Grid,
        ordering: Ordering.Index,
      };
    case ContainerType.Library:
      return {
        display: Display.Grid,
        ordering: Ordering.Title,
      };
    default:
      return {
        display: Display.Grid,
        ordering: Ordering.Title,
      };
  }
}

export function useListSetting(
  id: string,
  container: ContainerType,
): ListSetting {
  let settings = useSelector((storeState) => storeState.settings);
  return settings.listSettings[id] ?? defaultSetting(container);
}

const setMediaStore = createAction<{
  mediaStore: MediaStore;
  settings: SettingsState;
}>("setMediaStore");
export const clearMediaStore = createAction("clearMediaStore");
export const reportError = createAction<string>("reportError");
export const clearError = createAction("clearError");
export const setListSettings =
  createAction<[id: string, settings: ListSetting]>("setListSettings");
export const setDiscoveredServers = createAction<string[]>(
  "setDiscoveredServers",
);

const reducer = createReducer<StoreState>(
  {
    mediaStore: null,
    settings: { listSettings: {} },
    discoveredServers: [],
  },
  (builder) => {
    builder
      .addCase(setMediaStore, (state, { payload }) => {
        state.mediaStore = payload.mediaStore;
        state.settings = payload.settings;
      })
      .addCase(clearMediaStore, (state) => {
        state.mediaStore = null;
      })
      .addCase(reportError, (state, { payload }) => {
        state.notificationMessage = payload;
      })
      .addCase(clearError, (state) => {
        state.notificationMessage = undefined;
      })
      .addCase(setListSettings, (state, { payload: [id, settings] }) => {
        state.settings.listSettings[id] = settings;
      })
      .addCase(setDiscoveredServers, (state, { payload }) => {
        state.discoveredServers = payload;
      });
  },
);

const settingsPersist: Middleware<object, StoreState> =
  (store) => (next) => (action) => {
    let { mediaStore: oldStore, settings: oldSettings } = store.getState();
    next(action);
    let { mediaStore: newStore, settings: newSettings } = store.getState();

    if (oldStore !== newStore) {
      AsyncStorage.setItem(STORE_KEY, newStore?.location ?? "").catch(
        console.error,
      );
    } else if (newStore && oldSettings !== newSettings) {
      console.log("Persisting new settings");

      AsyncStorage.setItem(
        SETTINGS_KEY + newStore.location,
        JSON.stringify(newSettings),
      ).catch(console.error);
    }
  };

const store = configureStore({
  reducer,
  middleware: (getDefaultMiddleware) =>
    getDefaultMiddleware({
      serializableCheck: {
        ignoredPaths: ["mediaStore"],
        ignoredActions: ["setMediaStore"],
      },
    }).concat(settingsPersist),
});

async function loadSettings(storeLocation: string): Promise<SettingsState> {
  try {
    let settingsStr = await AsyncStorage.getItem(SETTINGS_KEY + storeLocation);

    if (settingsStr) {
      return JSON.parse(settingsStr) as SettingsState;
    }
  } catch (e) {
    console.error(e);
  }

  return {
    listSettings: {},
  };
}

export async function updateMediaStore(mediaStore: MediaStore) {
  await AsyncStorage.setItem(STORE_KEY, mediaStore.location);
  let settings = await loadSettings(mediaStore.location);
  store.dispatch(setMediaStore({ mediaStore, settings }));
}

async function initStore() {
  try {
    let storeLocation = await AsyncStorage.getItem(STORE_KEY);
    if (!storeLocation) {
      return;
    }

    let mediaStore = await MediaStore.loadStore(storeLocation);
    if (mediaStore) {
      await updateMediaStore(mediaStore);
    }
  } catch (e) {
    store.dispatch(reportError(String(e)));
  } finally {
    await SplashScreen.hideAsync();
  }
}

const initPromise = initStore();

export function StoreProvider({ children }: { children: ReactNode }) {
  use(initPromise);

  return <Provider store={store}>{children}</Provider>;
}
