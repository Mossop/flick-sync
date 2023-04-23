import { useMaterial3Theme } from "@pchmn/expo-material3-theme";
import {
  ReactNode,
  createContext,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import { useColorScheme } from "react-native";
import {
  MD3DarkTheme,
  MD3LightTheme,
  Provider as PaperProvider,
} from "react-native-paper";
import * as StatusBar from "expo-status-bar";
import * as NavigationBar from "expo-navigation-bar";

type Scheme = "dark" | "light";

const ThemeContext = createContext<(override: Scheme | null) => void>(() => {});

export function SchemeOverride({ scheme }: { scheme: Scheme }) {
  let setOverride = useContext(ThemeContext);

  useEffect(() => {
    setOverride(scheme);
    return () => setOverride(null);
  }, [scheme]);

  return null;
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  let [themeOverride, setThemeOverride] = useState<Scheme | null>(null);

  let colorScheme = useColorScheme();
  let { theme } = useMaterial3Theme();

  let isDark = (themeOverride ?? colorScheme) == "dark";

  let paperTheme = useMemo(
    () =>
      isDark
        ? { ...MD3DarkTheme, colors: theme.dark }
        : { ...MD3LightTheme, colors: theme.light },
    [isDark, theme],
  );

  useEffect(() => {
    StatusBar.setStatusBarBackgroundColor(paperTheme.colors.background, false);
    NavigationBar.setBackgroundColorAsync(paperTheme.colors.background);
    NavigationBar.setButtonStyleAsync(isDark ? "light" : "dark");
  }, [isDark, paperTheme]);

  return (
    <ThemeContext.Provider value={setThemeOverride}>
      <PaperProvider theme={paperTheme}>{children}</PaperProvider>
    </ThemeContext.Provider>
  );
}
