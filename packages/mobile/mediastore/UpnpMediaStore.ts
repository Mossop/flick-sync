import { isDownloaded, StateDecoder } from "../state";
import { Collection, Episode, Movie, Show, Video } from "../state/wrappers";
import { StateBasedMediaStore } from "./MediaStore";
import { ssdpDiscover } from "./ssdp";

export class UpnpMediaStore extends StateBasedMediaStore {
  static async listStores(): Promise<UpnpMediaStore[]> {
    let urls = await ssdpDiscover();

    let promises = urls.map((url) => UpnpMediaStore.init(url));
    let stores = await Promise.all(promises);
    return stores;
  }

  static async init(storeLocation: URL | string): Promise<UpnpMediaStore> {
    let response = await fetch(new URL("state.json", storeLocation));
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

    let response = await fetch(updateUrl, {
      method: "POST",
    });

    if (!response.ok) {
      console.warn(`Failed to persist playback state: HTTP ${response.status}`);
    }
  }
}
