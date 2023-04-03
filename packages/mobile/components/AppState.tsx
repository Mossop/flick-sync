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
import { SplashScreen } from "expo-router";
import { State } from "../modules/state";

const SETTINGS_KEY = "settings";

interface Settings {
  store: string;
}

interface ContextState {
  mediaState: State;
  settings: Settings;
}

class AppState {
  constructor(
    private contextState: ContextState,
    private contextSetter: Dispatch<SetStateAction<ContextState | undefined>>
  ) {}

  public get settings(): Settings {
    return this.contextState?.settings;
  }

  public get mediaState(): State {
    return this.contextState?.mediaState;
  }

  public path(path: string): string {
    return (
      this.settings.store +
      "/document/primary%3Aflicksync%2F" +
      encodeURIComponent(path)
    );
  }
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

  while (true) {
    let permission =
      await StorageAccessFramework.requestDirectoryPermissionsAsync(null);
    if (permission.granted) {
      console.log(`Got permission for ${permission.directoryUri}`);
      return {
        store: permission.directoryUri,
      };
    } else {
      console.log("Permission denied");
    }
  }
}

async function loadMediaState(store: string): Promise<State> {
  console.log(`Loading media state from ${store}`);
  try {
    let stateStr = await StorageAccessFramework.readAsStringAsync(
      store + "/document/primary%3Aflicksync%2F.flicksync.state.json"
    );
    return JSON.parse(stateStr);
  } catch (e) {
    console.error("State read failed", e);
  }

  return {
    clientId: "",
  };
}

async function init(): Promise<ContextState> {
  let settings = await loadSettings();

  return {
    settings,
    mediaState: await loadMediaState(settings.store),
  };
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
    return <SplashScreen />;
  }

  let appSettings = new AppState(state, setState);

  return (
    <AppStateContext.Provider value={appSettings}>
      {children}
    </AppStateContext.Provider>
  );
}
