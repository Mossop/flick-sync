import { Snackbar } from "react-native-paper";
import { useCallback } from "react";
import { useSettings } from "./AppState";

export default function Scrubber() {
  let settings = useSettings();

  let onDismiss = useCallback(() => {
    settings.notificationMessage = undefined;
  }, [settings]);

  let message = settings.notificationMessage;

  return (
    <Snackbar visible={message !== undefined} onDismiss={onDismiss}>
      {message}
    </Snackbar>
  );
}
