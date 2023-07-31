import { Snackbar } from "react-native-paper";
import { clearError, useAction, useSelector } from "./Store";

export default function Scrubber() {
  let onDismiss = useAction(clearError);
  let message = useSelector((storeState) => storeState.notificationMessage);

  return (
    <Snackbar visible={message !== undefined} onDismiss={onDismiss}>
      {message}
    </Snackbar>
  );
}
