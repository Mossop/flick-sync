import {
  createContext,
  Dispatch,
  ReactNode,
  SetStateAction,
  useContext,
  useState,
} from "react";
import AsyncStorage from "@react-native-async-storage/async-storage";
import { StorageAccessFramework } from "expo-file-system";
import * as SplashScreen from "expo-splash-screen";
import {
  isDownloaded,
  isMovieCollection,
  isMovieLibrary,
  ServerState,
  State,
  StateDecoder,
} from "../modules/state";

const SETTINGS_KEY = "settings";
const CONTENT_ROOT = "content://com.android.externalstorage.documents/tree/";

interface Settings {
  store: string;
}

interface ContextState {
  mediaState: State;
  settings: Settings;
}

function storagePath(store: string, path: string): string {
  let prefix = "/document";
  if (store.startsWith(CONTENT_ROOT)) {
    prefix += `/${store.substring(CONTENT_ROOT.length)}`;
  }

  return `${store}${prefix}${encodeURIComponent(`/${path}`)}`;
}

async function chooseStore(): Promise<string> {
  let permission =
    await StorageAccessFramework.requestDirectoryPermissionsAsync(null);

  if (permission.granted) {
    console.log(`Got permission for ${permission.directoryUri}`);
    return permission.directoryUri;
  }

  throw new Error("Permission denied");
}

async function loadSettings(): Promise<Settings> {
  console.log("Loading settings...");
  try {
    let settingsStr = await AsyncStorage.getItem(SETTINGS_KEY);
    console.log("Got settings", settingsStr);
    if (settingsStr) {
      return JSON.parse(settingsStr);
    }
  } catch (e) {
    console.error(e);
  }

  // eslint-disable-next-line no-constant-condition
  while (true) {
    try {
      let store = await chooseStore();

      let settings: Settings = {
        store,
      };

      await AsyncStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));

      return settings;
    } catch (e) {
      console.error(e);
    }
  }
}

function filterServers(servers: Record<string, ServerState>) {
  for (let [serverId, server] of Object.entries(servers)) {
    let empty = true;
    for (let [videoId, video] of Object.entries(server.videos)) {
      if (!video.parts.every((videoPart) => isDownloaded(videoPart.download))) {
        delete server.videos[videoId];
      } else {
        empty = false;
      }
    }

    if (empty) {
      // eslint-disable-next-line no-param-reassign
      delete servers[serverId];
      continue;
    }

    for (let [id, playlist] of Object.entries(server.playlists)) {
      playlist.videos = playlist.videos.filter(
        (video) => video.id in server.videos,
      );

      if (playlist.videos.length == 0) {
        delete server.playlists[id];
      }
    }

    for (let [id, season] of Object.entries(server.seasons)) {
      season.episodes = season.episodes.filter(
        (episode) => episode.id in server.videos,
      );

      if (season.episodes.length == 0) {
        delete server.seasons[id];
      }
    }

    for (let [id, show] of Object.entries(server.shows)) {
      show.seasons = show.seasons.filter(
        (season) => season.id in server.seasons,
      );

      if (show.seasons.length == 0) {
        delete server.shows[id];
      }
    }

    for (let [id, collection] of Object.entries(server.collections)) {
      if (isMovieCollection(collection)) {
        collection.items = collection.items.filter(
          (movie) => movie.id in server.videos,
        );

        if (collection.items.length == 0) {
          delete server.collections[id];
        }
      } else {
        collection.items = collection.items.filter(
          (show) => show.id in server.shows,
        );

        if (collection.items.length == 0) {
          delete server.collections[id];
        }
      }
    }

    for (let [id, library] of Object.entries(server.libraries)) {
      if (isMovieLibrary(library)) {
        library.collections = library.collections.filter(
          (collection) => collection.id in server.collections,
        );
        library.contents = library.contents.filter(
          (movie) => movie.id in server.videos,
        );

        if (library.contents.length == 0) {
          delete server.libraries[id];
        }
      } else {
        library.collections = library.collections.filter(
          (collection) => collection.id in server.collections,
        );
        library.contents = library.contents.filter(
          (show) => show.id in server.shows,
        );

        if (library.contents.length == 0) {
          delete server.libraries[id];
        }
      }
    }
  }
}

async function loadMediaState(store: string): Promise<State> {
  console.log(`Loading media state from ${store}`);
  try {
    let stateStr = await StorageAccessFramework.readAsStringAsync(
      storagePath(store, ".flicksync.state.json"),
    );

    let state = await StateDecoder.decodeToPromise(JSON.parse(stateStr));
    filterServers(state.servers);
    console.log(`Loaded state with ${state.servers.size} servers.`);
    return state;
  } catch (e) {
    console.error("State read failed", e);
  }

  return { servers: {} };
}

class AppState {
  constructor(
    private contextState: ContextState,
    private contextSetter: Dispatch<SetStateAction<ContextState | undefined>>,
  ) {}

  public get settings(): Settings {
    return this.contextState.settings;
  }

  public get mediaState(): State {
    return this.contextState.mediaState;
  }

  private async updateSettings(settings: Settings) {
    this.contextState = {
      ...this.contextState,
      settings,
    };

    await AsyncStorage.setItem(
      SETTINGS_KEY,
      JSON.stringify(this.contextState.settings),
    );
    this.contextSetter(this.contextState);
  }

  public async pickStore() {
    let store = await chooseStore();
    this.contextState.mediaState = await loadMediaState(store);
    this.updateSettings({ store });
  }

  public path(path: string): string {
    return storagePath(this.settings.store, path);
  }
}

async function init(): Promise<ContextState> {
  // Keep the splash screen visible while we fetch resources
  SplashScreen.preventAutoHideAsync();

  try {
    let settings = await loadSettings();

    return {
      settings,
      mediaState: await loadMediaState(settings.store),
    };
  } finally {
    SplashScreen.hideAsync();
  }
}

let deferredInit = init();

// @ts-ignore
const AppStateContext = createContext<AppState>(null);

export function useAppState(): AppState {
  return useContext(AppStateContext);
}

export function useSettings(): Settings {
  return useAppState().settings;
}

export function useMediaState(): State {
  return useAppState().mediaState;
}

export function AppStateProvider({ children }: { children: ReactNode }) {
  let [state, setState] = useState<ContextState>();

  if (!state) {
    deferredInit.then(setState);
    return null;
  }

  let appSettings = new AppState(state, setState);

  return (
    <AppStateContext.Provider value={appSettings}>
      {children}
    </AppStateContext.Provider>
  );
}
