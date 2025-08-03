import { JsonDecoder } from "ts.data.json";
import {
  CollectionState,
  DownloadState,
  EpisodeDetail,
  LibraryState,
  LibraryType,
  MovieDetail,
  PlaybackState,
  PlaylistState,
  SeasonState,
  ServerState,
  ShowState,
  State,
  RelatedFileState,
  VideoDetail,
  VideoPartState,
  VideoState,
} from "./base";

const RelatedFileStateDecoder = JsonDecoder.oneOf<RelatedFileState>(
  [
    JsonDecoder.object(
      {
        state: JsonDecoder.isExactly("none"),
      },
      "none",
    ),
    JsonDecoder.object(
      {
        state: JsonDecoder.isExactly("stored"),
        path: JsonDecoder.string,
        updated: JsonDecoder.number,
      },
      "stored",
    ),
  ],
  "RelatedFileState",
);

const DownloadStateDecoder = JsonDecoder.oneOf<DownloadState>(
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
);

const PlaybackStateDecoder = JsonDecoder.oneOf<PlaybackState>(
  [
    JsonDecoder.object(
      {
        state: JsonDecoder.isExactly("unplayed"),
      },
      "unplayed",
    ),
    JsonDecoder.object(
      {
        state: JsonDecoder.isExactly("inprogress"),
        position: JsonDecoder.number,
      },
      "inprogress",
    ),
    JsonDecoder.object(
      {
        state: JsonDecoder.isExactly("played"),
      },
      "played",
    ),
  ],
  "PlaybackState",
);

const VideoPartStateDecoder = JsonDecoder.object<VideoPartState>(
  {
    id: JsonDecoder.string,
    key: JsonDecoder.string,
    size: JsonDecoder.number,
    duration: JsonDecoder.number,
    download: DownloadStateDecoder,
  },
  "VideoPart",
);

const LibraryTypeDecoder = JsonDecoder.enumeration<LibraryType>(
  LibraryType,
  "LibraryState",
);

const LibraryStateDecoder = JsonDecoder.object<LibraryState>(
  {
    id: JsonDecoder.string,
    title: JsonDecoder.string,
    type: LibraryTypeDecoder,
  },
  "ShowLibraryState",
);

const ShowStateDecoder = JsonDecoder.object<ShowState>(
  {
    id: JsonDecoder.string,
    library: JsonDecoder.string,
    title: JsonDecoder.string,
    year: JsonDecoder.number,
    thumbnail: RelatedFileStateDecoder,
    lastUpdated: JsonDecoder.number,
    metadata: JsonDecoder.optional(RelatedFileStateDecoder),
  },
  "ShowState",
);

const SeasonStateDecoder = JsonDecoder.object<SeasonState>(
  {
    id: JsonDecoder.string,
    title: JsonDecoder.string,
    show: JsonDecoder.string,
    index: JsonDecoder.number,
  },
  "SeasonState",
);

const MovieDetailDecoder = JsonDecoder.object<MovieDetail>(
  {
    library: JsonDecoder.string,
    year: JsonDecoder.number,
  },
  "MovieState",
);

const EpisodeDetailDecoder = JsonDecoder.object<EpisodeDetail>(
  {
    season: JsonDecoder.string,
    index: JsonDecoder.number,
  },
  "EpisodeState",
);

const VideoDetailDecoder = JsonDecoder.oneOf<VideoDetail>(
  [MovieDetailDecoder, EpisodeDetailDecoder],
  "VideoState",
);

const VideoStateDecoder = JsonDecoder.object<VideoState>(
  {
    id: JsonDecoder.string,
    title: JsonDecoder.string,
    thumbnail: RelatedFileStateDecoder,
    airDate: JsonDecoder.string,
    mediaId: JsonDecoder.string,
    parts: JsonDecoder.array(VideoPartStateDecoder, "VideoPart[]"),
    detail: VideoDetailDecoder,
    transcodeProfile: JsonDecoder.optional(JsonDecoder.string),
    playbackState: PlaybackStateDecoder,
    lastUpdated: JsonDecoder.number,
    lastViewedAt: JsonDecoder.optional(JsonDecoder.number),
    metadata: JsonDecoder.optional(RelatedFileStateDecoder),
  },
  "VideoState",
);

const PlaylistStateDecoder = JsonDecoder.object<PlaylistState>(
  {
    id: JsonDecoder.string,
    title: JsonDecoder.string,
    videos: JsonDecoder.array(JsonDecoder.string, "PlaylistState.videos"),
  },
  "PlaylistState",
);

const CollectionStateDecoder = JsonDecoder.object<CollectionState>(
  {
    id: JsonDecoder.string,
    library: JsonDecoder.string,
    title: JsonDecoder.string,
    contents: JsonDecoder.array(JsonDecoder.string, "CollectionState.items"),
    thumbnail: RelatedFileStateDecoder,
    lastUpdated: JsonDecoder.number,
  },
  "CollectionState",
);

const ServerStateDecoder = JsonDecoder.object<ServerState>(
  {
    token: JsonDecoder.optional(JsonDecoder.string),
    name: JsonDecoder.string,
    libraries: JsonDecoder.optional(
      JsonDecoder.dictionary(LibraryStateDecoder, "ServerState.libraries"),
    ),
    playlists: JsonDecoder.optional(
      JsonDecoder.dictionary(PlaylistStateDecoder, "ServerState.playlists"),
    ),
    collections: JsonDecoder.optional(
      JsonDecoder.dictionary(CollectionStateDecoder, "ServerState.collections"),
    ),
    shows: JsonDecoder.optional(
      JsonDecoder.dictionary(ShowStateDecoder, "ServerState.shows"),
    ),
    seasons: JsonDecoder.optional(
      JsonDecoder.dictionary(SeasonStateDecoder, "ServerState.seasons"),
    ),
    videos: JsonDecoder.optional(
      JsonDecoder.dictionary(VideoStateDecoder, "ServerState.videos"),
    ),
  },
  "ServerState",
);

export const StateDecoder = JsonDecoder.object<State>(
  {
    schema: JsonDecoder.number,
    clientId: JsonDecoder.string,
    servers: JsonDecoder.dictionary(ServerStateDecoder, "State.servers"),
  },
  "State",
);
