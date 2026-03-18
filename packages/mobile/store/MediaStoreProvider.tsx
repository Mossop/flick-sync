import * as SplashScreen from "expo-splash-screen";
import {
  createContext,
  ReactNode,
  useContext,
  useEffect,
  useState,
} from "react";
import { useDispatch } from "react-redux";
import { MediaStore } from "./MediaStore";
import { DirectMediaStore } from "./DirectMediaStore";
import { setStoreLocation, loadSettings } from "../components/Store";

// Prevent splash screen from auto-hiding before init completes
SplashScreen.preventAutoHideAsync().catch(console.error);

interface MediaStoreContextValue {
  store: MediaStore;
}

const MediaStoreContext = createContext<MediaStoreContextValue | null>(null);

export function useMediaStore(): MediaStore {
  let ctx = useContext(MediaStoreContext);
  if (!ctx) {
    throw new Error("useMediaStore must be used inside MediaStoreProvider");
  }
  return ctx.store;
}

export function MediaStoreProvider({ children }: { children: ReactNode }) {
  let dispatch = useDispatch();
  let [contextValue, setContextValue] = useState<MediaStoreContextValue | null>(
    null,
  );

  useEffect(() => {
    let mediaStore = new DirectMediaStore();

    async function initialize() {
      await mediaStore.init();
      let settings = await loadSettings(mediaStore.location);
      dispatch(setStoreLocation({ location: mediaStore.location, settings }));
      setContextValue({ store: mediaStore });
      await SplashScreen.hideAsync();
    }

    initialize().catch(console.error);
  }, [dispatch]);

  if (!contextValue) {
    return null;
  }

  return (
    <MediaStoreContext.Provider value={contextValue}>
      {children}
    </MediaStoreContext.Provider>
  );
}
