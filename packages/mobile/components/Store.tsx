/* eslint-disable no-param-reassign */
import {
  Action,
  Dispatch,
  MiddlewareAPI,
  configureStore,
  createAction,
  createReducer,
} from "@reduxjs/toolkit";
import {
  Provider,
  useDispatch,
  useSelector as useReduxState,
} from "react-redux";
import { StorageAccessFramework, getInfoAsync } from "expo-file-system";
import * as SplashScreen from "expo-splash-screen";
import AsyncStorage from "@react-native-async-storage/async-storage";
import { ReactNode, useCallback } from "react";
import { PlaybackState, State } from "../state/base";
import { ContainerType, StateDecoder } from "../state";

const STORE_KEY = "store";
const SETTINGS_KEY = "settings#";
const STATE_FILE = ".flicksync.state.json";
const CONTENT_ROOT = "content://com.android.externalstorage.documents/tree/";

export enum Display {
  Grid = "grid",
  // eslint-disable-next-line @typescript-eslint/no-shadow
  List = "list",
}

export enum Ordering {
  Index = "index",
  Title = "title",
  AirDate = "airdate",
}

export interface ListSetting {
  display: Display;
  ordering: Ordering;
}

interface SettingsState {
  listSettings: Record<string, ListSetting>;
}

export interface StoreState {
  initialized: boolean;
  storeLocation: string;
  state: State;
  settings: SettingsState;
  notificationMessage?: string;
}

export function useSelector<Selected = unknown>(
  selector: (state: StoreState) => Selected,
) {
  return useReduxState<StoreState, Selected>(selector);
}

export function useAction<P>(
  action: (payload: P) => Action,
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

function storagePath(store: string, path: string): string {
  let prefix = "/document";
  if (store.startsWith(CONTENT_ROOT)) {
    prefix += `/${store.substring(CONTENT_ROOT.length)}`;
  }

  return `${store}${prefix}${encodeURIComponent(`/${path}`)}`;
}

export function useStoragePath(): (path: string) => string {
  let storeLocation = useSelector((storeState) => storeState.storeLocation);
  return useCallback(
    (path: string) => storagePath(storeLocation, path),
    [storeLocation],
  );
}

const init = createAction<Omit<StoreState, "initialized">>("init");
export const reportError = createAction<string>("reportError");
export const clearError = createAction("clearError");
export const setListSettings =
  createAction<[id: string, settings: ListSetting]>("setListSettings");
export const setPlaybackState =
  createAction<[server: string, id: string, playbackState: PlaybackState]>(
    "setPlaybackState",
  );

const reducer = createReducer<StoreState>(
  {
    initialized: false,
    storeLocation: "",
    state: { clientId: "" },
    settings: { listSettings: {} },
  },
  (builder) => {
    builder
      .addCase(init, (state, { payload }) => {
        state.initialized = true;
        state.storeLocation = payload.storeLocation;
        state.state = payload.state;
        state.settings = payload.settings;
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
      .addCase(
        setPlaybackState,
        (state, { payload: [serverId, id, playbackState] }) => {
          let server = state.state.servers?.[serverId];
          if (!server) {
            return;
          }

          let video = server.videos?.[id];
          if (!video) {
            return;
          }

          video.playbackState = playbackState;
        },
      );
  },
);

class StatePersister {
  private stateToPersist: State | undefined = undefined;

  private isPersisting: boolean = false;

  constructor(private store: string, private persistedState: State) {}

  public async persistState(state: State) {
    this.stateToPersist = state;

    if (this.isPersisting) {
      return;
    }

    this.isPersisting = true;
    try {
      while (this.persistedState !== this.stateToPersist) {
        await StorageAccessFramework.deleteAsync(
          storagePath(this.store, STATE_FILE),
          {
            idempotent: true,
          },
        );

        let file = await StorageAccessFramework.createFileAsync(
          this.store,
          STATE_FILE.substring(0, STATE_FILE.length - 5),
          "application/json",
        );

        let writingState: State = this.stateToPersist;
        await StorageAccessFramework.writeAsStringAsync(
          file,
          JSON.stringify(writingState, undefined, 2),
        );
        this.persistedState = writingState;
      }
    } catch (e) {
      console.error("Failed to persist state", e);
    } finally {
      this.isPersisting = false;
    }
  }
}

function statePersist(store: MiddlewareAPI<Dispatch<Action>, StoreState>) {
  let persister: StatePersister | null = null;

  return (next: Dispatch<Action>) => (action: Action) => {
    let { storeLocation: oldStoreLocation, state: oldState } = store.getState();
    next(action);
    let { storeLocation, state } = store.getState();

    if (!persister || storeLocation !== oldStoreLocation) {
      console.log(`Creating new state persister for ${storeLocation}`);
      persister = new StatePersister(storeLocation, state);
    } else if (state !== oldState) {
      console.log("Persisting new state");
      persister.persistState(state);
    }
  };
}

function settingsPersist(store: MiddlewareAPI<Dispatch<Action>, StoreState>) {
  return (next: Dispatch<Action>) => (action: Action) => {
    let { storeLocation: oldStoreLocation, settings: oldSettings } =
      store.getState();
    next(action);
    let { storeLocation, settings } = store.getState();

    if (oldStoreLocation === storeLocation && settings !== oldSettings) {
      console.log("Persisting new settings");
      AsyncStorage.setItem(
        SETTINGS_KEY + storeLocation,
        JSON.stringify(settings),
      );
    }
  };
}

const store = configureStore({
  reducer,
  middleware: (getDefaultMiddleware) =>
    getDefaultMiddleware().concat(statePersist).concat(settingsPersist),
});

async function loadSettings(storeLocation: string): Promise<SettingsState> {
  try {
    let settingsStr = await AsyncStorage.getItem(SETTINGS_KEY + storeLocation);

    if (settingsStr) {
      return JSON.parse(settingsStr);
    }
  } catch (e) {
    console.error(e);
  }

  return {
    listSettings: {},
  };
}

async function chooseStore(): Promise<string> {
  let permission =
    await StorageAccessFramework.requestDirectoryPermissionsAsync(null);

  if (!permission.granted) {
    throw new Error("Permission denied");
  }

  console.log(`Got permission for ${permission.directoryUri}`);

  try {
    let info = await getInfoAsync(
      storagePath(permission.directoryUri, STATE_FILE),
    );

    if (info.exists && !info.isDirectory) {
      return permission.directoryUri;
    }

    throw new Error(`Store is not a file`);
  } catch (e) {
    throw new Error(`Failed to access store: ${e}`);
  }
}

async function loadMediaState(storeLocation: string): Promise<State> {
  console.log(`Loading media state from ${storeLocation}`);
  try {
    let stateStr = await StorageAccessFramework.readAsStringAsync(
      storagePath(storeLocation, STATE_FILE),
    );

    let state = await StateDecoder.decodeToPromise(JSON.parse(stateStr));
    let servers = Object.values(state.servers ?? {});
    let videos = servers.flatMap((server) =>
      Object.values(server.videos ?? {}),
    );
    console.log(
      `Loaded state with ${servers.length} servers and ${videos.length} videos.`,
    );
    return state;
  } catch (e) {
    console.error("State read failed", e);
  }

  return { clientId: "", servers: {} };
}

async function findStore(): Promise<[string, State]> {
  let storeLocation: string | null = null;

  try {
    storeLocation = await AsyncStorage.getItem(STORE_KEY);

    if (storeLocation) {
      try {
        let info = await getInfoAsync(storagePath(storeLocation, STATE_FILE));

        if (!info.exists || info.isDirectory) {
          console.warn(`Previous state store is no longer a file.`);
          storeLocation = null;
        }
      } catch (e) {
        console.warn(`Failed to access previous store: ${e}`);
        storeLocation = null;
      }
    }
  } catch (e) {
    console.error(e);
  }

  // eslint-disable-next-line no-constant-condition
  while (true) {
    try {
      if (!storeLocation) {
        storeLocation = await chooseStore();
      }

      await AsyncStorage.setItem(STORE_KEY, storeLocation);

      let state = await loadMediaState(storeLocation);

      return [storeLocation, state];
    } catch (e) {
      console.error(e);
      storeLocation = null;
    }
  }
}

async function initStore() {
  // Keep the splash screen visible while we fetch resources
  SplashScreen.preventAutoHideAsync();

  try {
    let [storeLocation, state] = await findStore();
    let settings = await loadSettings(storeLocation);

    store.dispatch(
      init({
        storeLocation,
        settings,
        state,
      }),
    );
  } finally {
    SplashScreen.hideAsync();
  }
}

initStore().catch(console.error);

export async function pickNewStore() {
  try {
    let storeLocation = await chooseStore();
    let state = await loadMediaState(storeLocation);
    let settings = await loadSettings(storeLocation);

    store.dispatch(
      init({
        storeLocation,
        state,
        settings,
      }),
    );
  } catch (e) {
    store.dispatch(reportError(e.toString()));
  }
}

function EnsureInitialized({ children }: { children: ReactNode }) {
  let initialized = useSelector((state) => state.initialized);

  if (initialized) {
    return children;
  }
  return null;
}

export function StoreProvider({ children }: { children: ReactNode }) {
  return (
    <Provider store={store}>
      <EnsureInitialized>{children}</EnsureInitialized>
    </Provider>
  );
}
