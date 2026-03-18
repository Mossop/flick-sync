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
import AsyncStorage from "@react-native-async-storage/async-storage";
import { ReactNode, useCallback } from "react";
import { ContainerType } from "../state";

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
  storeLocation: string;
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

export const setStoreLocation = createAction<{
  location: string;
  settings: SettingsState;
}>("setStoreLocation");
export const reportError = createAction<string>("reportError");
export const clearError = createAction("clearError");
export const setListSettings =
  createAction<[id: string, settings: ListSetting]>("setListSettings");

const reducer = createReducer<StoreState>(
  {
    storeLocation: "",
    settings: { listSettings: {} },
  },
  (builder) => {
    builder
      .addCase(setStoreLocation, (state, { payload }) => {
        state.storeLocation = payload.location;
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
      });
  },
);

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
    getDefaultMiddleware().concat(settingsPersist),
});

export async function loadSettings(
  storeLocation: string,
): Promise<SettingsState> {
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

export function StoreProvider({ children }: { children: ReactNode }) {
  return <Provider store={store}>{children}</Provider>;
}
