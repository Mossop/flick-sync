import {
  createContext,
  Dispatch,
  ReactNode,
  SetStateAction,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import AsyncStorage from "@react-native-async-storage/async-storage";
import { getInfoAsync, StorageAccessFramework } from "expo-file-system";
import * as SplashScreen from "expo-splash-screen";
import { MediaState, StateDecoder } from "../state";
import { State } from "../state/base";

const SETTINGS_KEY = "settings";
const CONTENT_ROOT = "content://com.android.externalstorage.documents/tree/";
const STATE_FILE = ".flicksync.state.json";

interface SettingsState {
  store: string;
}

interface ContextState {
  state: State;
  settings: SettingsState;
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

async function loadSettings(): Promise<SettingsState> {
  console.log("Loading settings...");

  let settings: SettingsState = { store: "" };

  try {
    let settingsStr = await AsyncStorage.getItem(SETTINGS_KEY);
    console.log("Got settings", settingsStr);
    if (settingsStr) {
      settings = JSON.parse(settingsStr);
    }
  } catch (e) {
    console.error(e);
  }

  if (settings.store) {
    try {
      let info = await getInfoAsync(storagePath(settings.store, STATE_FILE));

      if (info.exists && !info.isDirectory) {
        return settings;
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
      settings = {
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
      storagePath(store, STATE_FILE),
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

class Settings {
  public constructor(
    private state: SettingsState,
    private setContextState: Dispatch<SetStateAction<ContextState | undefined>>,
  ) {}

  private async persistSettings(newContextState: ContextState) {
    await AsyncStorage.setItem(
      SETTINGS_KEY,
      JSON.stringify(newContextState.settings),
    );
    this.setContextState(newContextState);
  }

  public get store(): string {
    return this.state.store;
  }

  public path(path: string): string {
    return storagePath(this.store, path);
  }

  public async pickStore() {
    let store = await chooseStore();
    let state = await loadMediaState(store);
    this.persistSettings({ state, settings: { store } });
  }
}

class AppManager {
  private stateToPersist: State;

  private isPersisting: boolean = false;

  constructor(private settings: Settings, private persistedState: State) {
    this.stateToPersist = persistedState;
  }

  public async persistState(state: State) {
    this.stateToPersist = state;
    if (this.isPersisting) {
      return;
    }

    this.isPersisting = true;
    try {
      while (this.persistedState !== this.stateToPersist) {
        await StorageAccessFramework.deleteAsync(
          this.settings.path(STATE_FILE),
          {
            idempotent: true,
          },
        );

        let file = await StorageAccessFramework.createFileAsync(
          this.settings.store,
          STATE_FILE.substring(0, STATE_FILE.length - 5),
          "application/json",
        );

        console.log("Writing new state");
        let writingState = this.stateToPersist;
        await StorageAccessFramework.writeAsStringAsync(
          file,
          JSON.stringify(writingState, undefined, 2),
        );
        this.persistedState = writingState;
      }
    } finally {
      this.isPersisting = false;
    }
  }
}

async function init(): Promise<ContextState> {
  // Keep the splash screen visible while we fetch resources
  SplashScreen.preventAutoHideAsync();

  try {
    let settings = await loadSettings();

    return { settings, state: await loadMediaState(settings.store) };
  } finally {
    SplashScreen.hideAsync();
  }
}

let deferredInit = init();

// @ts-ignore
const AppStateContext = createContext<[MediaState, Settings]>(null);

export function useSettings(): Settings {
  return useContext(AppStateContext)[1];
}

export function useMediaState(): MediaState {
  return useContext(AppStateContext)[0];
}

function ProviderInner({
  contextState,
  setContextState,
  children,
}: {
  contextState: ContextState;
  setContextState: Dispatch<SetStateAction<ContextState | undefined>>;
  children: ReactNode;
}) {
  let settings = useMemo(
    () => new Settings(contextState.settings, setContextState),
    [contextState.settings, setContextState],
  );

  let appState = useMemo(
    () => new AppManager(settings, contextState.state),
    [settings, contextState.state],
  );

  let mediaState = useMemo(
    () =>
      new MediaState(contextState.state, (state) => {
        setContextState({
          ...contextState,
          state,
        });
      }),
    [contextState, setContextState],
  );

  useEffect(() => {
    appState.persistState(contextState.state);
  }, [appState, contextState.state]);

  let providerValue = useMemo<[MediaState, Settings]>(
    () => [mediaState, settings],
    [mediaState, settings],
  );

  return (
    <AppStateContext.Provider value={providerValue}>
      {children}
    </AppStateContext.Provider>
  );
}

export function AppStateProvider({ children }: { children: ReactNode }) {
  let [contextState, setContextState] = useState<ContextState>();

  useEffect(() => {
    deferredInit.then(setContextState);
  }, []);

  if (!contextState) {
    return null;
  }

  return (
    <ProviderInner
      contextState={contextState}
      setContextState={setContextState}
    >
      {children}
    </ProviderInner>
  );
}
