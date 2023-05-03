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
} from "./base";
import { Replace } from "../modules/types";

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
  protected constructor(
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
      .filter((vid) => vid.library === this);
  }

  public collections(): readonly MovieCollection[] {
    return this.server
      .collections()
      .filter(isMovieCollection)
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
      .filter(isMovie);
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
    return this.state.contents.map((id) => this.server.getShow(id));
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
    return this.state.videos.map((id) => this.server.getVideo(id));
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
    return this.server.seasons().filter((season) => season.show === this);
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
      .filter((ep) => ep.season === this);
  }
}

abstract class VideoWapper<S extends Omit<VideoState, "detail">>
  extends ServerItemWrapper<S>
  implements IVideo
{
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

  public get playPosition(): number | undefined {
    return this.state.playPosition;
  }

  public abstract get library(): Library;
}

export class Episode
  extends VideoWapper<Replace<VideoState, { detail: EpisodeDetail }>>
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
  extends VideoWapper<Replace<VideoState, { detail: MovieDetail }>>
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
): () => R[] {
  let result: R[] | null = null;

  return function getter(this: any): R[] {
    if (result !== null) {
      return result;
    }

    let items = (this.state[key] ?? {}) as Record<string, any>;
    result = Object.keys(items).map((id) => itemLookup(this, id));
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

  public libraries = listGetter("libraries", (server, id) =>
    server.getLibrary(id),
  );

  public collections = listGetter("collections", (server, id) =>
    server.getCollection(id),
  );

  public playlists = listGetter("playlists", (server, id) =>
    server.getPlaylist(id),
  );

  public shows = listGetter("shows", (server, id) => server.getShow(id));

  public seasons = listGetter("seasons", (server, id) => server.getSeason(id));

  public videos = listGetter("videos", (server, id) => server.getVideo(id));
}

export class MediaState extends StateWrapper<State> {
  private static wrappers = new WeakMap<State, MediaState>();

  public static wrap(state: State, setState: Dispatch<State>): MediaState {
    let ms = MediaState.wrappers.get(state);
    if (ms) {
      return ms;
    }

    ms = new MediaState(state, setState);
    MediaState.wrappers.set(state, ms);
    return ms;
  }

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
