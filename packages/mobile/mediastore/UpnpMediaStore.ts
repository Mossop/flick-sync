import { isDownloaded, StateDecoder } from "../state";
import { Collection, Episode, Movie, Show, Video } from "../state/wrappers";
import { StateBasedMediaStore } from "./MediaStore";

const NETWORK_TIMEOUT = 5000;

function errorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }

  return String(error);
}

async function fetchWithTimeout(
  input: URL | RequestInfo,
  init?: RequestInit,
): Promise<Response> {
  let controller = new AbortController();
  let timeout = setTimeout(() => controller.abort(), NETWORK_TIMEOUT);

  try {
    return await fetch(input, {
      ...init,
      signal: controller.signal,
    });
  } finally {
    clearTimeout(timeout);
  }
}

export class UpnpMediaStore extends StateBasedMediaStore {
  static async init(storeLocation: URL | string): Promise<UpnpMediaStore> {
    let response: Response;
    try {
      response = await fetchWithTimeout(new URL("state.json", storeLocation));
    } catch (error) {
      if (error instanceof Error && error.name === "AbortError") {
        throw new Error(`Timed out fetching state from ${storeLocation}`);
      }

      throw new Error(
        `Failed to connect to ${storeLocation}: ${errorMessage(error)}`,
      );
    }

    if (!response.ok) {
      throw new Error(
        `Failed to fetch state from ${storeLocation}: ${response.status}`,
      );
    }

    let json = await response.json();
    let result = StateDecoder.decode(json);
    if (!result.isOk()) {
      throw new Error(`Invalid state: ${result.error}`);
    }

    return new UpnpMediaStore(result.value, storeLocation.toString());
  }

  private resolveUri(path: string): URL {
    return new URL(path, this.location);
  }

  thumbnailUri(item: Video | Show | Collection): string | undefined {
    if (item.thumbnail.state != "stored") {
      return undefined;
    }

    let itemType: string;
    if (item instanceof Episode || item instanceof Movie) {
      itemType = "video";
    } else if (item instanceof Show) {
      itemType = "show";
    } else {
      itemType = "collection";
    }

    return this.resolveUri(
      `thumbnail/${item.server.id}/${itemType}/${item.id}`,
    ).toString();
  }

  videoUri(video: Video): string | undefined {
    if (!isDownloaded(video.download)) {
      return undefined;
    }

    return this.resolveUri(`stream/${video.server.id}/${video.id}`).toString();
  }

  async persistPlaybackState(video: Video) {
    console.log(
      `Persisting playback state for ${video.id} to ${video.playPosition}`,
    );

    let updateUrl = this.resolveUri(
      `/playback/${video.server.id}/${video.library.id}/video/${video.id}`,
    );
    updateUrl.searchParams.append(
      "position",
      String(video.playPosition / 1000),
    );

    let response: Response;
    try {
      response = await fetchWithTimeout(updateUrl, {
        method: "POST",
      });
    } catch (error) {
      if (error instanceof Error && error.name === "AbortError") {
        console.warn("Timed out persisting playback state");
        return;
      }

      console.warn(
        `Error while persisting playback state: ${errorMessage(error)}`,
      );

      return;
    }

    if (!response.ok) {
      console.warn(`Failed to persist playback state: HTTP ${response.status}`);
    }
  }
}
