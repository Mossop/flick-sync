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
import type { ListSetting } from "./List";

const SETTINGS_KEY = "settings";
const CONTENT_ROOT = "content://com.android.externalstorage.documents/tree/";
const STATE_FILE = ".flicksync.state.json";

interface SettingsState {
  store: string;
  listSettings: Record<string, ListSetting>;
}

interface ContextState {
  state: State;
  settings: SettingsState;
  notificationMessage?: string;
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

  let settings: SettingsState = { store: "", listSettings: {} };

  try {
    let settingsStr = await AsyncStorage.getItem(SETTINGS_KEY);
    console.log("Got settings", settingsStr);
    if (settingsStr) {
      Object.assign(settings, JSON.parse(settingsStr));
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
        listSettings: {},
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

class Settings {
  public constructor(
    private state: ContextState,
    private setContextState: Dispatch<SetStateAction<ContextState>>,
  ) {}

  private async persistSettings(newContextState: ContextState) {
    await AsyncStorage.setItem(
      SETTINGS_KEY,
      JSON.stringify(newContextState.settings),
    );
    this.setContextState(newContextState);
  }

  public get store(): string {
    return this.state.settings.store;
  }

  public path(path: string): string {
    return storagePath(this.store, path);
  }

  public getListSetting(id: string): ListSetting | undefined {
    return this.state.settings.listSettings[id];
  }

  public setListSetting(id: string, setting: ListSetting) {
    let state: ContextState = {
      state: this.state.state,
      settings: {
        ...this.state.settings,
        listSettings: {
          ...this.state.settings.listSettings,
          [id]: setting,
        },
      },
    };

    this.persistSettings(state);
  }

  public async pickStore() {
    let store = await chooseStore();
    let state = await loadMediaState(store);
    this.persistSettings({ state, settings: { store, listSettings: {} } });
  }

  public get notificationMessage(): string | undefined {
    return this.state.notificationMessage;
  }

  public set notificationMessage(notificationMessage: string | undefined) {
    this.setContextState({
      ...this.state,
      notificationMessage,
    });
  }
}

class StatePersister {
  private persistedState: State | undefined = undefined;

  private stateToPersist: State | undefined = undefined;

  private isPersisting: boolean = false;

  constructor(private store: string) {}

  public async persistState(state: State) {
    this.stateToPersist = state;

    if (!this.persistedState) {
      this.persistedState = this.stateToPersist;
      return;
    }

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

        console.log("Writing new state");
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
  setContextState: Dispatch<SetStateAction<ContextState>>;
  children: ReactNode;
}) {
  let settings = useMemo(
    () => new Settings(contextState, setContextState),
    [contextState, setContextState],
  );

  let statePersister = useMemo(
    () => new StatePersister(settings.store),
    [settings.store],
  );

  let mediaState = useMemo(
    () =>
      new MediaState(contextState.state, (state) => {
        setContextState((prev) => ({
          ...prev,
          state,
        }));
      }),
    [contextState.state, setContextState],
  );

  useEffect(() => {
    statePersister.persistState(contextState.state);
  }, [statePersister, contextState.state]);

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
      setContextState={
        setContextState as Dispatch<SetStateAction<ContextState>>
      }
    >
      {children}
    </ProviderInner>
  );
}
