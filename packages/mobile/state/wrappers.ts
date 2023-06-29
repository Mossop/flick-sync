import { Dispatch } from "react";
import {
  ShowState,
  SeasonState,
  LibraryState,
  MovieDetail,
  VideoState,
  EpisodeDetail,
  ServerState,
  PlaylistState,
  State,
  CollectionState,
  ThumbnailState,
  VideoPartState,
  LibraryType,
  PlaybackState,
} from "./base";
import { Replace } from "../modules/types";

function any<R>(items: readonly R[], cb: (item: R) => boolean): boolean {
  return !items.every((item: R) => !cb(item));
}

function memo<T, K, R>(fn: (this: T, id: K) => R): (this: T, id: K) => R {
  let cache = new Map<K, R>();

  return function inner(this: T, id: K): R {
    let result = cache.get(id);
    if (result) {
      return result;
    }

    result = fn.call(this, id);
    cache.set(id, result);
    return result;
  };
}

function videoIsDownloaded(video: Video): boolean {
  return video.isDownloaded;
}

function videosHaveDownloads(videos: readonly Video[]): boolean {
  return any(videos, videoIsDownloaded);
}

function seasonHasDownloads(season: Season): boolean {
  return videosHaveDownloads(season.episodes);
}

function showHasDownloads(show: Show): boolean {
  return any(show.seasons, seasonHasDownloads);
}

export function isMovie(v: Video): v is Movie {
  // eslint-disable-next-line @typescript-eslint/no-use-before-define
  return v instanceof Movie;
}

export function isEpisode(v: Video): v is Episode {
  return !isMovie(v);
}

export function isMovieLibrary(l: Library): l is MovieLibrary {
  // eslint-disable-next-line @typescript-eslint/no-use-before-define
  return l instanceof MovieLibrary;
}

export function isShowLibrary(l: Library): l is ShowLibrary {
  return !isMovieLibrary(l);
}

export function isMovieCollection(c: Collection): c is MovieCollection {
  // eslint-disable-next-line @typescript-eslint/no-use-before-define
  return c instanceof MovieCollection;
}

export function isShowCollection(c: Collection): c is ShowCollection {
  return !isMovieCollection(c);
}

type WrapperInterface<T, V = {}> = Replace<Readonly<T>, V>;

type IServer = Readonly<
  Omit<
    ServerState,
    "playlists" | "libraries" | "shows" | "seasons" | "collections" | "videos"
  >
> & {
  readonly id: string;
};

type ILibrary = WrapperInterface<
  Omit<LibraryState, "type">,
  {
    readonly server: Server;
    readonly contents: readonly (Show | Movie)[];
    readonly collections: () => readonly Collection[];
  }
>;

type IMovieLibrary = Replace<
  ILibrary,
  {
    readonly contents: readonly Movie[];
    readonly collections: () => readonly MovieCollection[];
  }
>;

type IShowLibrary = Replace<
  ILibrary,
  {
    readonly contents: readonly Show[];
    readonly collections: () => readonly ShowCollection[];
  }
>;

type ICollection = WrapperInterface<
  CollectionState,
  {
    readonly server: Server;
    readonly library: Library;
    readonly contents: readonly (Show | Movie)[];
  }
>;

type IMovieCollection = Replace<
  ICollection,
  {
    readonly contents: readonly Movie[];
  }
>;

type IShowCollection = Replace<
  ICollection,
  {
    readonly contents: readonly Show[];
  }
>;

type IPlaylist = WrapperInterface<
  PlaylistState,
  {
    readonly server: Server;
    readonly videos: readonly Video[];
  }
>;

type IShow = WrapperInterface<
  ShowState,
  {
    readonly server: Server;
    readonly library: ShowLibrary;
    readonly seasons: readonly Season[];
  }
>;

type ISeason = WrapperInterface<
  SeasonState,
  {
    readonly server: Server;
    readonly show: Show;
    readonly library: ShowLibrary;
    readonly episodes: readonly Episode[];
  }
>;

type IVideo = WrapperInterface<
  Omit<VideoState, "detail">,
  {
    readonly server: Server;
    readonly library: Library;
    playPosition: number;
  }
>;

type IMovie = Replace<
  IVideo & Readonly<MovieDetail>,
  {
    readonly library: MovieLibrary;
  }
>;

type IEpisode = Replace<
  IVideo & Readonly<EpisodeDetail>,
  {
    readonly library: ShowLibrary;
    readonly season: Season;
  }
>;

abstract class StateWrapper<S> {
  public constructor(
    protected readonly state: S,
    protected readonly setState: Dispatch<S>,
  ) {}
}

abstract class ServerItemWrapper<S> extends StateWrapper<S> {
  public constructor(
    public readonly server: Server,
    state: S,
    setState: Dispatch<S>,
  ) {
    super(state, setState);
  }
}

abstract class LibraryWrapper
  extends ServerItemWrapper<LibraryState>
  implements ILibrary
{
  public get id(): string {
    return this.state.id;
  }

  public get title(): string {
    return this.state.title;
  }

  public abstract get contents(): (Show | Movie)[];

  public abstract collections(): readonly Collection[];
}

export class MovieLibrary extends LibraryWrapper implements IMovieLibrary {
  public get contents(): Movie[] {
    return this.server
      .videos()
      .filter(isMovie)
      .filter(videoIsDownloaded)
      .filter((vid) => vid.library === this);
  }

  public collections(): readonly MovieCollection[] {
    return this.server
      .collections()
      .filter(isMovieCollection)
      .filter((collection) => videosHaveDownloads(collection.contents))
      .filter((col) => col.library === this);
  }
}

export class ShowLibrary extends LibraryWrapper implements IShowLibrary {
  public get contents(): Show[] {
    return this.server.shows().filter((show) => show.library === this);
  }

  public collections(): readonly ShowCollection[] {
    return this.server
      .collections()
      .filter(isShowCollection)
      .filter((collection) => any(collection.contents, showHasDownloads))
      .filter((col) => col.library === this);
  }
}

export abstract class CollectionWrapper
  extends ServerItemWrapper<CollectionState>
  implements ICollection
{
  public get id(): string {
    return this.state.id;
  }

  public get title(): string {
    return this.state.title;
  }

  public get thumbnail(): ThumbnailState {
    return this.state.thumbnail;
  }

  public get lastUpdated(): number {
    return this.state.lastUpdated;
  }

  public abstract get library(): Library;

  public abstract get contents(): readonly (Show | Movie)[];
}

export class MovieCollection
  extends CollectionWrapper
  implements IMovieCollection
{
  public get library(): MovieLibrary {
    return this.server.getLibrary(this.state.library) as MovieLibrary;
  }

  public get contents(): readonly Movie[] {
    return this.state.contents
      .map((id) => this.server.getVideo(id))
      .filter(isMovie)
      .filter(videoIsDownloaded);
  }
}

export class ShowCollection
  extends CollectionWrapper
  implements IShowCollection
{
  public get library(): ShowLibrary {
    return this.server.getLibrary(this.state.library) as ShowLibrary;
  }

  public get contents(): readonly Show[] {
    return this.state.contents
      .map((id) => this.server.getShow(id))
      .filter(showHasDownloads);
  }
}

export class Playlist
  extends ServerItemWrapper<PlaylistState>
  implements IPlaylist
{
  public get id(): string {
    return this.state.id;
  }

  public get title(): string {
    return this.state.title;
  }

  public get videos(): readonly Video[] {
    return this.state.videos
      .map((id) => this.server.getVideo(id))
      .filter(videoIsDownloaded);
  }
}

export class Show extends ServerItemWrapper<ShowState> implements IShow {
  public get id(): string {
    return this.state.id;
  }

  public get title(): string {
    return this.state.title;
  }

  public get lastUpdated(): number {
    return this.state.lastUpdated;
  }

  public get year(): number {
    return this.state.year;
  }

  public get thumbnail(): ThumbnailState {
    return this.state.thumbnail;
  }

  public get library(): ShowLibrary {
    return this.server.getLibrary(this.state.library) as ShowLibrary;
  }

  public get seasons(): readonly Season[] {
    return this.server
      .seasons()
      .filter((season) => season.show === this)
      .filter(seasonHasDownloads);
  }
}

export class Season extends ServerItemWrapper<SeasonState> implements ISeason {
  public get id(): string {
    return this.state.id;
  }

  public get title(): string {
    return this.state.title;
  }

  public get index(): number {
    return this.state.index;
  }

  public get library(): ShowLibrary {
    return this.show.library;
  }

  public get show(): Show {
    return this.server.getShow(this.state.show);
  }

  public get episodes(): readonly Episode[] {
    return this.server
      .videos()
      .filter(isEpisode)
      .filter((ep) => ep.season === this)
      .filter(videoIsDownloaded);
  }
}

abstract class VideoWrapper<S extends Omit<VideoState, "detail">>
  extends ServerItemWrapper<S>
  implements IVideo
{
  public readonly totalDuration: number;

  public constructor(server: Server, state: S, setState: Dispatch<S>) {
    super(server, state, setState);

    this.totalDuration = state.parts.reduce(
      (total, part) => total + part.duration,
      0,
    );
  }

  public get id(): string {
    return this.state.id;
  }

  public get title(): string {
    return this.state.title;
  }

  public get airDate(): string {
    return this.state.airDate;
  }

  public get thumbnail(): ThumbnailState {
    return this.state.thumbnail;
  }

  public get mediaId(): string {
    return this.state.mediaId;
  }

  public get lastUpdated(): number {
    return this.state.lastUpdated;
  }

  public get parts(): VideoPartState[] {
    return this.state.parts;
  }

  public get transcodeProfile(): string | undefined {
    return this.state.transcodeProfile;
  }

  public get playbackState(): PlaybackState {
    return this.state.playbackState;
  }

  public set playbackState(playbackState: PlaybackState) {
    this.setState({
      ...this.state,
      playbackState,
    });
  }

  public get playPosition(): number {
    if (this.playbackState.state == "inprogress") {
      return this.playbackState.position;
    }

    if (this.playbackState.state == "played") {
      return this.totalDuration;
    }

    return 0;
  }

  public set playPosition(position: number) {
    this.playbackState = { state: "inprogress", position };
  }

  public get isDownloaded(): boolean {
    return this.parts.every(
      (part) =>
        part.download.state == "downloaded" ||
        part.download.state == "transcoded",
    );
  }

  public abstract get library(): Library;
}

export class Episode
  extends VideoWrapper<Replace<VideoState, { detail: EpisodeDetail }>>
  implements IEpisode
{
  public get library(): ShowLibrary {
    return this.season.library;
  }

  public get season(): Season {
    return this.server.getSeason(this.state.detail.season);
  }

  public get index(): number {
    return this.state.detail.index;
  }
}

export class Movie
  extends VideoWrapper<Replace<VideoState, { detail: MovieDetail }>>
  implements IMovie
{
  public get library(): MovieLibrary {
    return this.server.getLibrary(this.state.detail.library) as MovieLibrary;
  }

  public get year(): number {
    return this.state.detail.year;
  }
}

export type Video = Episode | Movie;
export type Library = MovieLibrary | ShowLibrary;
export type Collection = MovieCollection | ShowCollection;

export function isVideo(item: any): item is Video {
  return item instanceof VideoWrapper;
}

export function isLibrary(item: any): item is Library {
  return item instanceof LibraryWrapper;
}

export function isCollection(item: any): item is Collection {
  return item instanceof CollectionWrapper;
}

function itemGetter<R>(
  key: string,
  factory: (server: any, state: any, setter: Dispatch<any>) => R,
) {
  return memo(function getter(this: any, id: string): R {
    let items = this.state[key] as Record<string, any> | undefined;
    let itemState = (items ?? {})[id];
    if (!itemState) {
      throw new Error(`Unknown ${key} ${id}`);
    }

    return factory(this, itemState, (newState) => {
      this.setState({
        ...this.state,
        [key]: {
          ...(this.state[key] ?? {}),
          [id]: newState,
        },
      });
    });
  });
}

function listGetter<R>(
  key: string,
  itemLookup: (server: Server, id: string) => R,
  itemFilter: (item: R) => boolean = () => true,
): () => R[] {
  let result: R[] | null = null;

  return function getter(this: any): R[] {
    if (result !== null) {
      return result;
    }

    let items = (this.state[key] ?? {}) as Record<string, any>;
    result = Object.keys(items).map((id) => itemLookup(this, id));
    if (itemFilter) {
      result = result.filter(itemFilter);
    }

    return result;
  };
}

function clsFactory<S, R>(
  Cls: new (server: Server, state: S, setState: Dispatch<S>) => R,
): (server: Server, state: S, setState: Dispatch<S>) => R {
  return (server: Server, state: S, setState: Dispatch<S>) =>
    new Cls(server, state, setState);
}

export class Server extends StateWrapper<ServerState> implements IServer {
  public constructor(
    public readonly id: string,
    state: ServerState,
    setState: Dispatch<ServerState>,
  ) {
    super(state, setState);
  }

  public get name(): string {
    return this.state.name;
  }

  public getLibrary = itemGetter(
    "libraries",
    (
      server: Server,
      state: LibraryState,
      setState: Dispatch<LibraryState>,
    ): Library => {
      if (state.type == LibraryType.Movie) {
        return new MovieLibrary(server, state, setState);
      }
      return new ShowLibrary(server, state, setState);
    },
  );

  public getCollection = itemGetter(
    "collections",
    (
      server: Server,
      state: CollectionState,
      setState: Dispatch<CollectionState>,
    ): Collection => {
      let library = server.getLibrary(state.library);
      if (library instanceof MovieLibrary) {
        return new MovieCollection(server, state, setState);
      }
      return new ShowCollection(server, state, setState);
    },
  );

  public getPlaylist = itemGetter("playlists", clsFactory(Playlist));

  public getShow = itemGetter("shows", clsFactory(Show));

  public getSeason = itemGetter("seasons", clsFactory(Season));

  public getVideo = itemGetter(
    "videos",
    (
      server: Server,
      state: VideoState,
      setState: Dispatch<VideoState>,
    ): Video => {
      if ("library" in state.detail) {
        // @ts-ignore
        return new Movie(server, state, setState);
      }
      // @ts-ignore
      return new Episode(server, state, setState);
    },
  );

  public libraries = listGetter(
    "libraries",
    (server, id) => server.getLibrary(id),
    (library) => {
      if (library instanceof ShowLibrary) {
        return any(library.contents, showHasDownloads);
      }
      return videosHaveDownloads(library.contents);
    },
  );

  public collections = listGetter(
    "collections",
    (server, id) => server.getCollection(id),
    (collection) => {
      if (collection instanceof ShowCollection) {
        return any(collection.contents, showHasDownloads);
      }
      return videosHaveDownloads(collection.contents);
    },
  );

  public playlists = listGetter(
    "playlists",
    (server, id) => server.getPlaylist(id),
    (playlist) => videosHaveDownloads(playlist.videos),
  );

  public shows = listGetter(
    "shows",
    (server, id) => server.getShow(id),
    showHasDownloads,
  );

  public seasons = listGetter(
    "seasons",
    (server, id) => server.getSeason(id),
    seasonHasDownloads,
  );

  public videos = listGetter(
    "videos",
    (server, id) => server.getVideo(id),
    (video) => video.isDownloaded,
  );
}

export class MediaState extends StateWrapper<State> {
  getServer = memo(function getServer(this: MediaState, id: string): Server {
    let ss = this.state.servers?.[id];
    if (!ss) {
      throw new Error(`Unknown server ${id}`);
    }

    return new Server(id, ss, (newState) => {
      this.setState({
        ...this.state,
        servers: {
          ...this.state.servers,
          [id]: newState,
        },
      });
    });
  });

  public servers(): Server[] {
    return Object.keys(this.state.servers ?? {}).map((id) =>
      this.getServer(id),
    );
  }
}
