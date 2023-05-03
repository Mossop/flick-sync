import { DownloadState } from "./base";

export { ThumbnailState, DownloadState } from "./base";
export { StateDecoder } from "./decoders";
export * from "./wrappers";

export function isDownloaded(
  ds: DownloadState,
): ds is { state: "downloaded" | "transcoded"; path: string } {
  return ds.state == "downloaded" || ds.state == "transcoded";
}
