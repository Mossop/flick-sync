import { JsonDecoder } from "ts.data.json";
import * as RustState from "./ruststate";

type Replace<T, V> = Omit<T, keyof V> & V;

export type ThumbnailState =
  | { state: "none" }
  | { state: "downloaded"; path: string };

const ThumbnailStateDecoder = JsonDecoder.failover(
  { state: "none" },
  JsonDecoder.oneOf<ThumbnailState>(
    [
      JsonDecoder.object(
        {
          state: JsonDecoder.isExactly("none"),
        },
        "none"
      ),
      JsonDecoder.object(
        {
          state: JsonDecoder.isExactly("downloaded"),
          path: JsonDecoder.string,
        },
        "downloaded"
      ),
    ],
    "ThumbnailState"
  )
);

export type DownloadState =
  | { state: "none" }
  | { state: "downloading"; path: string }
  | { state: "transcoding"; path: string }
  | { state: "downloaded"; path: string }
  | { state: "transcoded"; path: string };

const DownloadStateDecoder = JsonDecoder.failover(
  { state: "none" },
  JsonDecoder.oneOf<DownloadState>(
    [
      JsonDecoder.object(
        {
          state: JsonDecoder.isExactly("none"),
        },
        "none"
      ),
      JsonDecoder.object(
        {
          state: JsonDecoder.isExactly("downloading"),
          path: JsonDecoder.string,
        },
        "downloading"
      ),
      JsonDecoder.object(
        {
          state: JsonDecoder.isExactly("transcoding"),
          path: JsonDecoder.string,
        },
        "transcoding"
      ),
      JsonDecoder.object(
        {
          state: JsonDecoder.isExactly("downloaded"),
          path: JsonDecoder.string,
        },
        "downloaded"
      ),
      JsonDecoder.object(
        {
          state: JsonDecoder.isExactly("transcoded"),
          path: JsonDecoder.string,
        },
        "transcoded"
      ),
    ],
    "DownloadState"
  )
);

export type CollectionState = Replace<
  RustState.CollectionState,
  {
    items: number[];
    thumbnail: ThumbnailState;
  }
>;

const CollectionStateDecoder = JsonDecoder.object<CollectionState>(
  {
    id: JsonDecoder.number,
    library: JsonDecoder.number,
    title: JsonDecoder.string,
    items: JsonDecoder.failover(
      [],
      JsonDecoder.array(JsonDecoder.number, "number[]")
    ),
    thumbnail: ThumbnailStateDecoder,
  },
  "CollectionState"
);

export type PlaylistState = Replace<
  RustState.PlaylistState,
  {
    videos: number[];
  }
>;

const PlaylistStateDecoder = JsonDecoder.object<PlaylistState>(
  {
    id: JsonDecoder.number,
    title: JsonDecoder.string,
    videos: JsonDecoder.failover(
      [],
      JsonDecoder.array(JsonDecoder.number, "number[]")
    ),
  },
  "PlaylistState"
);

enum LibraryType {
  Movie = "movie",
  Show = "show",
}

const LibraryTypeDecoder = JsonDecoder.enumeration<LibraryType>(
  LibraryType,
  "LibraryType"
);

export type LibraryState = Replace<
  RustState.LibraryState,
  {
    type: LibraryType;
  }
>;

const LibraryStateDecoder = JsonDecoder.object<LibraryState>(
  {
    id: JsonDecoder.number,
    title: JsonDecoder.string,
    type: LibraryTypeDecoder,
  },
  "LibraryState"
);

export type SeasonState = RustState.SeasonState;

const SeasonStateDecoder = JsonDecoder.object<SeasonState>(
  {
    id: JsonDecoder.number,
    title: JsonDecoder.string,
    show: JsonDecoder.number,
    index: JsonDecoder.number,
  },
  "SeasonState"
);

export type ShowState = Replace<
  RustState.ShowState,
  {
    thumbnail: ThumbnailState;
  }
>;

const ShowStateDecoder = JsonDecoder.object<ShowState>(
  {
    id: JsonDecoder.number,
    title: JsonDecoder.string,
    library: JsonDecoder.number,
    year: JsonDecoder.number,
    thumbnail: ThumbnailStateDecoder,
  },
  "ShowState"
);

export type MovieState = RustState.MovieState;

const MovieStateDecoder = JsonDecoder.object<MovieState>(
  {
    library: JsonDecoder.number,
    year: JsonDecoder.number,
  },
  "MovieState"
);

export type EpisodeState = RustState.EpisodeState;

const EpisodeStateDecoder = JsonDecoder.object<EpisodeState>(
  {
    season: JsonDecoder.number,
    index: JsonDecoder.number,
  },
  "EpisodeState"
);

export type VideoDetail = MovieState | EpisodeState;

const VideoDetailDecoder = JsonDecoder.oneOf<VideoDetail>(
  [MovieStateDecoder, EpisodeStateDecoder],
  "VideoDetail"
);

export type VideoState = Replace<
  RustState.VideoState,
  {
    detail: VideoDetail;
    thumbnail: ThumbnailState;
    download: DownloadState;
  }
>;

const VideoStateDecoder = JsonDecoder.object<VideoState>(
  {
    id: JsonDecoder.number,
    title: JsonDecoder.string,
    thumbnail: ThumbnailStateDecoder,
    download: DownloadStateDecoder,
    detail: VideoDetailDecoder,
  },
  "VideoState"
);

export type ServerState = Replace<
  Omit<RustState.ServerState, "token">,
  {
    playlists: PlaylistState[];
    collections: CollectionState[];
    libraries: LibraryState[];
    shows: ShowState[];
    seasons: RustState.SeasonState[];
    videos: VideoState[];
  }
>;

const ServerStateDecoder = JsonDecoder.object<ServerState>(
  {
    name: JsonDecoder.string,
    playlists: JsonDecoder.failover(
      [],
      JsonDecoder.array(PlaylistStateDecoder, "PlaylistState[]")
    ),
    collections: JsonDecoder.failover(
      [],
      JsonDecoder.array(CollectionStateDecoder, "CollectionState[]")
    ),
    libraries: JsonDecoder.failover(
      [],
      JsonDecoder.array(LibraryStateDecoder, "LibraryState[]")
    ),
    shows: JsonDecoder.failover(
      [],
      JsonDecoder.array(ShowStateDecoder, "ShowState[]")
    ),
    seasons: JsonDecoder.failover(
      [],
      JsonDecoder.array(SeasonStateDecoder, "SeasonState[]")
    ),
    videos: JsonDecoder.failover(
      [],
      JsonDecoder.array(VideoStateDecoder, "VideoState[]")
    ),
  },
  "ServerState"
);

export type State = Replace<
  Omit<RustState.State, "clientId">,
  {
    servers: Record<string, ServerState>;
  }
>;

export const StateDecoder = JsonDecoder.object<State>(
  {
    servers: JsonDecoder.failover(
      {},
      JsonDecoder.dictionary(ServerStateDecoder, "Record<string, ServerState>")
    ),
  },
  "State"
);
