import { DownloadState } from "./base";

export { DownloadState } from "./base";
export { StateDecoder } from "./decoders";
export * from "./wrappers";

export function isDownloaded(
  ds: DownloadState,
): ds is { state: "downloaded" | "transcoded"; path: string } {
  return ds.state == "downloaded" || ds.state == "transcoded";
}

export enum ContainerType {
  // eslint-disable-next-line @typescript-eslint/no-shadow
  MovieCollection,
  // eslint-disable-next-line @typescript-eslint/no-shadow
  ShowCollection,
  // eslint-disable-next-line @typescript-eslint/no-shadow
  Playlist,
  // eslint-disable-next-line @typescript-eslint/no-shadow
  Show,
  Library,
}
