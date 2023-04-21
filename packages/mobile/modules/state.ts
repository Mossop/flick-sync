import { JsonDecoder } from "ts.data.json";
import * as RustState from "./ruststate";

type Replace<T, V> = Omit<T, keyof V> & V;

function optional<T>(
  failover: T,
  decoder: JsonDecoder.Decoder<T>,
): JsonDecoder.Decoder<T> {
  return JsonDecoder.optional(decoder).map(
    (val: T | undefined) => val ?? failover,
  );
}

function optionalArray<T>(
  decoder: JsonDecoder.Decoder<T>,
): JsonDecoder.Decoder<T[]> {
  return optional([], JsonDecoder.array(decoder, "[]"));
}

function decode<T>(decoder: JsonDecoder.Decoder<T>, val: any): T {
  let result = decoder.decode(val);
  if (result.isOk()) {
    return result.value;
  }
  throw new Error(result.error);
}

function getOrThrow<K, V>(map: Map<K, V>, key: K, error: string): V {
  let val = map.get(key);
  if (val !== undefined) {
    return val;
  }

  throw new Error(error);
}

function mapIndex<I, V>(
  decoder: JsonDecoder.Decoder<I>,
  map: Map<I, V>,
  error: string,
): JsonDecoder.Decoder<V> {
  return decoder.map((id) => getOrThrow(map, id, `${error}: ${id}`));
}

function mapped<T extends { id: any }>(items: T[]): Map<T["id"], T> {
  return new Map(items.map((item: T): [T["id"], T] => [item.id, item]));
}

export type ThumbnailState =
  | { state: "none" }
  | { state: "downloaded"; path: string };

const ThumbnailStateDecoder = optional(
  { state: "none" },
  JsonDecoder.oneOf<ThumbnailState>(
    [
      JsonDecoder.object(
        {
          state: JsonDecoder.isExactly("none"),
        },
        "none",
      ),
      JsonDecoder.object(
        {
          state: JsonDecoder.isExactly("downloaded"),
          path: JsonDecoder.string,
        },
        "downloaded",
      ),
    ],
    "ThumbnailState",
  ),
);

export type DownloadState =
  | { state: "none" }
  | { state: "downloading"; path: string }
  | { state: "transcoding"; path: string }
  | { state: "downloaded"; path: string }
  | { state: "transcoded"; path: string };

export function isDownloaded(
  ds: DownloadState,
): ds is { state: "downloaded" | "transcoded"; path: string } {
  return ds.state == "downloaded" || ds.state == "transcoded";
}

const DownloadStateDecoder = optional(
  { state: "none" },
  JsonDecoder.oneOf<DownloadState>(
    [
      JsonDecoder.object(
        {
          state: JsonDecoder.isExactly("none"),
        },
        "none",
      ),
      JsonDecoder.object(
        {
          state: JsonDecoder.isExactly("downloading"),
          path: JsonDecoder.string,
        },
        "downloading",
      ),
      JsonDecoder.object(
        {
          state: JsonDecoder.isExactly("transcoding"),
          path: JsonDecoder.string,
        },
        "transcoding",
      ),
      JsonDecoder.object(
        {
          state: JsonDecoder.isExactly("downloaded"),
          path: JsonDecoder.string,
        },
        "downloaded",
      ),
      JsonDecoder.object(
        {
          state: JsonDecoder.isExactly("transcoded"),
          path: JsonDecoder.string,
        },
        "transcoded",
      ),
    ],
    "DownloadState",
  ),
);

export type ShowCollectionState = Replace<
  Omit<RustState.CollectionState, "lastUpdated">,
  {
    library: ShowLibraryState;
    items: ShowState[];
    thumbnail: ThumbnailState;
  }
>;

export type MovieCollectionState = Replace<
  ShowCollectionState,
  {
    library: MovieLibraryState;
    items: MovieState[];
  }
>;

export type CollectionState = MovieCollectionState | ShowCollectionState;

export type PlaylistState = Replace<
  RustState.PlaylistState,
  {
    server: ServerState;
    videos: VideoState[];
  }
>;

export enum LibraryType {
  Movie = "movie",
  Show = "show",
}

export type MovieLibraryState = Replace<
  RustState.LibraryState,
  {
    server: ServerState;
    type: LibraryType.Movie;
    contents: MovieState[];
    collections: MovieCollectionState[];
  }
>;

export type ShowLibraryState = Replace<
  MovieLibraryState,
  {
    type: LibraryType.Show;
    contents: ShowState[];
    collections: ShowCollectionState[];
  }
>;

export type LibraryState = ShowLibraryState | MovieLibraryState;

export function isMovieLibrary(l: LibraryState): l is MovieLibraryState {
  return l.type == LibraryType.Movie;
}

export function isShowLibrary(l: LibraryState): l is ShowLibraryState {
  return l.type == LibraryType.Show;
}

export function isMovieCollection(
  c: CollectionState,
): c is MovieCollectionState {
  return isMovieLibrary(c.library);
}

export function isShowCollection(c: CollectionState): c is ShowCollectionState {
  return isShowLibrary(c.library);
}

export type SeasonState = Replace<
  RustState.SeasonState,
  {
    show: ShowState;
    episodes: EpisodeState[];
  }
>;

export type ShowState = Replace<
  Omit<RustState.ShowState, "lastUpdated">,
  {
    library: ShowLibraryState;
    thumbnail: ThumbnailState;
    seasons: SeasonState[];
  }
>;

export type MovieDetail = Replace<
  RustState.MovieState,
  {
    library: MovieLibraryState;
  }
>;

export type EpisodeDetail = Replace<
  RustState.EpisodeState,
  {
    season: SeasonState;
  }
>;

export type VideoPartState = Replace<
  RustState.VideoPartState,
  { download: DownloadState }
>;

const VideoPartStateDecoder = JsonDecoder.object<VideoPartState>(
  {
    duration: JsonDecoder.number,
    download: DownloadStateDecoder,
  },
  "VideoPart",
);

export type VideoDetail = MovieDetail | EpisodeDetail;

export type MovieState = Replace<
  Omit<RustState.VideoState, "lastUpdated" | "mediaId">,
  {
    detail: MovieDetail;
    thumbnail: ThumbnailState;
    parts: VideoPartState[];
  }
>;

export type EpisodeState = Replace<
  MovieState,
  {
    detail: EpisodeDetail;
  }
>;

export type VideoState = MovieState | EpisodeState;

export function isMovie(v: VideoState): v is MovieState {
  return "library" in v.detail;
}

export function isEpisode(v: VideoState): v is EpisodeState {
  return !isMovie(v);
}

export function videoLibrary(video: VideoState): LibraryState {
  if (isMovie(video)) {
    return video.detail.library;
  }
  return video.detail.season.show.library;
}

export type ServerState = Replace<
  Omit<RustState.ServerState, "token">,
  {
    id: string;
    playlists: Map<string, PlaylistState>;
    collections: Map<string, CollectionState>;
    libraries: Map<number, LibraryState>;
    shows: Map<string, ShowState>;
    seasons: Map<string, SeasonState>;
    videos: Map<string, VideoState>;
  }
>;

function decodeServerState(json: any): ServerState {
  if (json === null || json === undefined) {
    throw new Error(`Unexpected server state '${json}'`);
  }

  if (typeof json != "object") {
    throw new Error(`Unexpected server state type '${typeof json}'`);
  }

  let serverState: ServerState = {
    id: "",
    name: decode(JsonDecoder.string, json.name),
    playlists: new Map(),
    collections: new Map(),
    libraries: new Map(),
    shows: new Map(),
    seasons: new Map(),
    videos: new Map(),
  };

  const ShowLibraryStateDecoder = JsonDecoder.object<ShowLibraryState>(
    {
      id: JsonDecoder.number,
      title: JsonDecoder.string,
      type: JsonDecoder.isExactly(LibraryType.Show),
      server: JsonDecoder.constant(serverState),
      contents: JsonDecoder.constant([]),
      collections: JsonDecoder.constant([]),
    },
    "ShowLibraryState",
  );

  const MovieLibraryStateDecoder = JsonDecoder.object<MovieLibraryState>(
    {
      id: JsonDecoder.number,
      title: JsonDecoder.string,
      type: JsonDecoder.isExactly(LibraryType.Movie),
      server: JsonDecoder.constant(serverState),
      contents: JsonDecoder.constant([]),
      collections: JsonDecoder.constant([]),
    },
    "MovieLibraryState",
  );

  const LibraryStateDecoder = JsonDecoder.oneOf<LibraryState>(
    [MovieLibraryStateDecoder, ShowLibraryStateDecoder],
    "LibraryState",
  );

  serverState.libraries = decode(
    optionalArray(LibraryStateDecoder).map(mapped),
    json.libraries,
  );

  const ShowStateDecoder = JsonDecoder.object<ShowState>(
    {
      id: JsonDecoder.string,
      title: JsonDecoder.string,
      library: mapIndex(
        JsonDecoder.number,
        serverState.libraries,
        "Unknown library",
      ).map((l) => {
        if (isShowLibrary(l)) {
          return l;
        }
        throw new Error("Invalid library type");
      }),
      year: JsonDecoder.number,
      thumbnail: ThumbnailStateDecoder,
      seasons: JsonDecoder.constant([]),
    },
    "ShowState",
  );

  serverState.shows = decode(
    optionalArray(ShowStateDecoder).map(mapped),
    json.shows,
  );

  for (let show of serverState.shows.values()) {
    show.library.contents.push(show);
  }

  const SeasonStateDecoder = JsonDecoder.object<SeasonState>(
    {
      id: JsonDecoder.string,
      title: JsonDecoder.string,
      show: mapIndex(JsonDecoder.string, serverState.shows, "Unknown show"),
      index: JsonDecoder.number,
      episodes: JsonDecoder.constant([]),
    },
    "SeasonState",
  );

  serverState.seasons = decode(
    optionalArray(SeasonStateDecoder).map(mapped),
    json.seasons,
  );

  for (let season of serverState.seasons.values()) {
    season.show.seasons.push(season);
  }

  const MovieDetailDecoder = JsonDecoder.object<MovieDetail>(
    {
      library: mapIndex(
        JsonDecoder.number,
        serverState.libraries,
        "Unknown library",
      ).map((l) => {
        if (isMovieLibrary(l)) {
          return l;
        }
        throw new Error("Invalid library type");
      }),
      year: JsonDecoder.number,
    },
    "MovieState",
  );

  const EpisodeDetailDecoder = JsonDecoder.object<EpisodeDetail>(
    {
      season: mapIndex(
        JsonDecoder.string,
        serverState.seasons,
        `Unknown season`,
      ),
      index: JsonDecoder.number,
    },
    "EpisodeState",
  );

  const MovieStateDecoder = JsonDecoder.object<MovieState>(
    {
      id: JsonDecoder.string,
      title: JsonDecoder.string,
      thumbnail: ThumbnailStateDecoder,
      airDate: JsonDecoder.string,
      parts: JsonDecoder.array(VideoPartStateDecoder, "VideoPart[]"),
      detail: MovieDetailDecoder,
      transcodeProfile: JsonDecoder.optional(JsonDecoder.string),
    },
    "MovieState",
  );

  const EpisodeStateDecoder = JsonDecoder.object<EpisodeState>(
    {
      id: JsonDecoder.string,
      title: JsonDecoder.string,
      thumbnail: ThumbnailStateDecoder,
      airDate: JsonDecoder.string,
      parts: JsonDecoder.array(VideoPartStateDecoder, "VideoPart[]"),
      detail: EpisodeDetailDecoder,
      transcodeProfile: JsonDecoder.optional(JsonDecoder.string),
    },
    "EpisodeState",
  );

  const VideoStateDecoder = JsonDecoder.oneOf<VideoState>(
    [MovieStateDecoder, EpisodeStateDecoder],
    "VideoState",
  );

  serverState.videos = decode(
    optionalArray(VideoStateDecoder).map(mapped),
    json.videos,
  );

  for (let video of serverState.videos.values()) {
    if (isMovie(video)) {
      video.detail.library.contents.push(video);
    } else {
      video.detail.season.episodes.push(video);
    }
  }

  const MovieCollectionStateDecoder = JsonDecoder.object<MovieCollectionState>(
    {
      id: JsonDecoder.string,
      library: mapIndex(
        JsonDecoder.number,
        serverState.libraries,
        "Unknown library",
      ).map((l) => {
        if (isMovieLibrary(l)) {
          return l;
        }
        throw new Error("Invalid library type");
      }),
      title: JsonDecoder.string,
      items: optionalArray(
        JsonDecoder.string.map((id) => {
          let val = serverState.videos.get(id);
          if (val === undefined || isEpisode(val)) {
            throw new Error(`Unknown collection item '${id}'`);
          }

          return val;
        }),
      ),
      thumbnail: ThumbnailStateDecoder,
    },
    "MovieCollectionState",
  );

  const ShowCollectionStateDecoder = JsonDecoder.object<ShowCollectionState>(
    {
      id: JsonDecoder.string,
      library: mapIndex(
        JsonDecoder.number,
        serverState.libraries,
        "Unknown library",
      ).map((l) => {
        if (isShowLibrary(l)) {
          return l;
        }
        throw new Error("Invalid library type");
      }),
      title: JsonDecoder.string,
      items: optionalArray(
        JsonDecoder.string.map((id) => {
          let val = serverState.shows.get(id);
          if (val === undefined) {
            throw new Error(`Unknown collection item '${id}'`);
          }

          return val;
        }),
      ),
      thumbnail: ThumbnailStateDecoder,
    },
    "ShowCollectionState",
  );

  const CollectionStateDecoder = JsonDecoder.oneOf<CollectionState>(
    [MovieCollectionStateDecoder, ShowCollectionStateDecoder],
    "CollectionState",
  );

  serverState.collections = decode(
    optionalArray(CollectionStateDecoder).map(mapped),
    json.collections,
  );

  for (let collection of serverState.collections.values()) {
    if (isMovieCollection(collection)) {
      collection.library.collections.push(collection);
    } else {
      collection.library.collections.push(collection);
    }
  }

  const PlaylistStateDecoder = JsonDecoder.object<PlaylistState>(
    {
      id: JsonDecoder.string,
      title: JsonDecoder.string,
      server: JsonDecoder.constant(serverState),
      videos: optionalArray(
        mapIndex(JsonDecoder.string, serverState.videos, "Unknown video"),
      ),
    },
    "PlaylistState",
  );

  serverState.playlists = decode(
    optionalArray(PlaylistStateDecoder).map(mapped),
    json.playlists,
  );

  return serverState;
}

export type State = Replace<
  Omit<RustState.State, "clientId">,
  {
    servers: Map<string, ServerState>;
  }
>;

export const StateDecoder = JsonDecoder.object<State>(
  {
    servers: optional(
      new Map(),
      JsonDecoder.dictionary(
        JsonDecoder.succeed.map(decodeServerState),
        "Record<string, ServerState>",
      ).map(
        (rec) =>
          new Map(
            Object.entries(rec).map(([id, ss]) => {
              // eslint-disable-next-line no-param-reassign
              ss.id = id;
              return [id, ss];
            }),
          ),
      ),
    ),
  },
  "State",
);
