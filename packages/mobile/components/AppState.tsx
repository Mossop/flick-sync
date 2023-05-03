import {
  createContext,
  Dispatch,
  ReactNode,
  SetStateAction,
  useContext,
  useState,
} from "react";
import AsyncStorage from "@react-native-async-storage/async-storage";
import { getInfoAsync, StorageAccessFramework } from "expo-file-system";
import * as SplashScreen from "expo-splash-screen";
import { MediaState, StateDecoder } from "../state";
import { State } from "../state/base";
import { Replace } from "../modules/types";

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

  let partialSettings: Replace<Settings, { store?: string }> = {};

  try {
    let settingsStr = await AsyncStorage.getItem(SETTINGS_KEY);
    console.log("Got settings", settingsStr);
    if (settingsStr) {
      partialSettings = JSON.parse(settingsStr);
    }
  } catch (e) {
    console.error(e);
  }

  if (partialSettings.store) {
    try {
      let info = await getInfoAsync(
        storagePath(partialSettings.store, ".flicksync.state.json"),
      );

      if (info.exists && !info.isDirectory) {
        return partialSettings as Settings;
      }

      console.warn(`Previous state store is no longer a file.`);
    } catch (e) {
      console.warn(`Failed to access previous store: ${e}`);
    }
  }

  // eslint-disable-next-line no-constant-condition
  while (true) {
    try {
      let store = await chooseStore();
      let settings: Settings = {
        ...partialSettings,
        store,
      };

      await AsyncStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));

      return settings;
    } catch (e) {
      console.error(e);
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
    let servers = Object.values(state.servers ?? {});
    let videos = servers.flatMap((server) => Object.values(server.videos));
    console.log(
      `Loaded state with ${servers.length} servers and ${videos.length} videos.`,
    );
    return state;
  } catch (e) {
    console.error("State read failed", e);
  }

  return { clientId: "", servers: {} };
}

class AppState {
  constructor(
    private contextState: ContextState,
    private contextSetter: Dispatch<SetStateAction<ContextState | undefined>>,
  ) {}

  public get settings(): Settings {
    return this.contextState.settings;
  }

  public get mediaState(): MediaState {
    return MediaState.wrap(
      this.contextState.mediaState,
      (mediaState: State) => {
        let contextState: ContextState = {
          ...this.contextState,
          mediaState,
        };

        this.contextSetter(contextState);
      },
    );
  }

  private async updateSettings(settings: Settings) {
    let contextState: ContextState = {
      ...this.contextState,
      settings,
    };

    await AsyncStorage.setItem(
      SETTINGS_KEY,
      JSON.stringify(contextState.settings),
    );
    this.contextSetter(contextState);
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

export function useMediaState(): MediaState {
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
