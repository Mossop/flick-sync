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
  MovieCollection,
  ShowCollection,
  Playlist,
  Show,
  Library,
}
