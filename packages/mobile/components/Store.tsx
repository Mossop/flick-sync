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
import {
  FileInfo,
  StorageAccessFramework,
  getInfoAsync,
} from "expo-file-system/legacy";
import * as SplashScreen from "expo-splash-screen";
import AsyncStorage from "@react-native-async-storage/async-storage";
import { ReactNode, useCallback } from "react";
import { PlaybackState, PlaybackUpdates, State } from "../state/base";
import { ContainerType, StateDecoder } from "../state";
import { PlaybackUpdatesDecoder } from "../state/decoders";

const STORE_KEY = "store";
const SETTINGS_KEY = "settings#";
const STATE_FILE = ".flicksync.state.json";
const STATE_BACKUP_FILE = ".flicksync.state.json.backup";
const PLAYBACK_FILE = ".flicksync.playback.json";
const CONTENT_ROOT = "content://com.android.externalstorage.documents/tree/";

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

function extractPlaybackUpdates(state: State): PlaybackUpdates {
  let servers: Record<string, Record<string, PlaybackState>> = {};

  for (let [serverId, server] of Object.entries(state.servers ?? {})) {
    let videos: Record<string, PlaybackState> = {};

    for (let [videoId, video] of Object.entries(server.videos ?? {})) {
      videos[videoId] = video.playbackState;
    }

    if (Object.keys(videos).length > 0) {
      servers[serverId] = videos;
    }
  }
  return { servers };
}

async function safeInfo(path: string): Promise<FileInfo | undefined> {
  try {
    return await getInfoAsync(path);
  } catch (e) {
    console.warn(`Failed to get file metadata for ${path}`, e);
    return undefined;
  }
}

class StatePersister {
  private playbackToPersist: PlaybackUpdates | undefined = undefined;

  private isPersisting = false;

  constructor(private store: string) {}

  public async persistPlayback(state: State) {
    if (Object.keys(state.servers ?? {}).length === 0) {
      console.warn("Refusing to persist playback for empty state");
      return;
    }

    this.playbackToPersist = extractPlaybackUpdates(state);

    if (this.isPersisting) {
      return;
    }

    this.isPersisting = true;
    try {
      while (this.playbackToPersist !== undefined) {
        let path = storagePath(this.store, PLAYBACK_FILE);
        let info = await safeInfo(path);

        let writingUpdates = this.playbackToPersist;
        this.playbackToPersist = undefined;

        let data = JSON.stringify(writingUpdates, undefined, 2);

        if (!info?.exists) {
          await StorageAccessFramework.createFileAsync(
            this.store,
            PLAYBACK_FILE.substring(0, PLAYBACK_FILE.length - 5),
            "application/json",
          );
        } else if (data.length < info.size) {
          // Writes to existing files don't truncate the file so pad out the
          // data to write to the current size of the file.
          data += " ".repeat(info.size - data.length);
        }

        await StorageAccessFramework.writeAsStringAsync(path, data);
      }
    } catch (e) {
      console.error("Failed to persist playback", e);
    } finally {
      this.isPersisting = false;
    }
  }
}

const statePersist: Middleware<object, StoreState> = (store) => {
  let persister: StatePersister | null = null;

  return (next) => (action) => {
    let { storeLocation: oldStoreLocation } = store.getState();
    next(action);
    let { storeLocation, state } = store.getState();

    if (!persister || storeLocation !== oldStoreLocation) {
      console.log(`Creating new state persister for ${storeLocation}`);
      persister = new StatePersister(storeLocation);
    } else if (setPlaybackState.match(action)) {
      console.log("Persisting playback state");
      persister.persistPlayback(state).catch(console.error);
    }
  };
};

const settingsPersist: Middleware<object, StoreState> =
  (store) => (next) => (action) => {
    let { storeLocation: oldStoreLocation, settings: oldSettings } =
      store.getState();
    next(action);
    let { storeLocation, settings } = store.getState();

    if (oldStoreLocation === storeLocation && settings !== oldSettings) {
      console.log("Persisting new settings");

      AsyncStorage.setItem(
        SETTINGS_KEY + storeLocation,
        JSON.stringify(settings),
      ).catch(console.error);
    }
  };

const store = configureStore({
  reducer,
  middleware: (getDefaultMiddleware) =>
    getDefaultMiddleware().concat(statePersist).concat(settingsPersist),
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

async function applyPlaybackStates(storeLocation: string, state: State) {
  // Merge any pending playback updates written since the last Rust sync.
  try {
    let playbackStr = await StorageAccessFramework.readAsStringAsync(
      storagePath(storeLocation, PLAYBACK_FILE),
    );

    let json = JSON.parse(playbackStr);
    let result = PlaybackUpdatesDecoder.decode(json);
    if (!result.isOk()) {
      throw new Error(`Invalid state: ${result.error}`);
    }

    let updates = result.value;

    for (let [serverId, videos] of Object.entries(updates.servers ?? {})) {
      for (let [videoId, playbackState] of Object.entries(videos)) {
        let video = state.servers?.[serverId]?.videos?.[videoId];
        if (video) {
          video.playbackState = playbackState;
        }
      }
    }
    console.log("Merged pending playback updates");
  } catch {
    // Playback file missing or unreadable — not an error.
  }
}

async function loadMediaState(storeLocation: string): Promise<State> {
  console.log(`Loading media state from ${storeLocation}`);

  for (const file of [STATE_FILE, STATE_BACKUP_FILE]) {
    try {
      let stateStr = await StorageAccessFramework.readAsStringAsync(
        storagePath(storeLocation, file),
      );

      let json: State = JSON.parse(stateStr);
      let result = StateDecoder.decode(json);
      if (!result.isOk()) {
        throw new Error(`Invalid state: ${result.error}`);
      }

      let state = result.value;

      let servers = Object.values(state.servers ?? {});
      let videos = servers.flatMap((server) =>
        Object.values(server.videos ?? {}),
      );
      console.log(
        `Loaded state from ${file} with ${servers.length} servers and ${videos.length} videos.`,
      );

      await applyPlaybackStates(storeLocation, state);

      return state;
    } catch (e) {
      console.error(`State read failed from ${file}`, e);
    }
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

  while (true) {
    try {
      storeLocation ??= await chooseStore();

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
  await SplashScreen.preventAutoHideAsync();

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
    await SplashScreen.hideAsync();
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
    store.dispatch(reportError(String(e)));
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
