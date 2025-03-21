use std::{
    cmp::Ordering,
    future::{Ready, ready},
    io::SeekFrom,
    str::FromStr,
    sync::Mutex,
};

use actix_web::{
    FromRequest, HttpRequest, HttpResponse,
    body::SizedStream,
    get,
    http::header,
    post,
    web::{Data, Path, Query, ThinData},
};
use bytes::Bytes;
use flick_sync::{Collection, FlickSync, Library, LibraryType, PlaybackState, Season, Show, Video};
use futures::TryStreamExt;
use rinja::Template;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncSeekExt, BufReader},
    sync::broadcast::Sender,
};
use tokio_stream::wrappers::BroadcastStream;
use tokio_util::io::ReaderStream;
use tracing::error;

use crate::{
    EmbeddedFileStream, Resources,
    serve::{
        ConnectionInfo, Event, SyncStatus,
        events::{ProgressBarTemplate, SyncLogTemplate, SyncProgressBar},
    },
    shared::{StreamLimiter, uniform_title},
};

struct HxTarget(Option<String>);

impl FromRequest for HxTarget {
    type Error = actix_web::Error;

    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(
        req: &actix_web::HttpRequest,
        _payload: &mut actix_web::dev::Payload,
    ) -> Self::Future {
        let target = match req.headers().get("HX-Target") {
            Some(v) => match v.to_str() {
                Ok(s) => HxTarget(Some(s.to_owned())),
                _ => HxTarget(None),
            },
            None => HxTarget(None),
        };

        ready(Ok(target))
    }
}

fn render<T: Template>(template: T) -> HttpResponse {
    match template.render() {
        Ok(body) => HttpResponse::Ok()
            .append_header(header::ContentType(mime::TEXT_HTML))
            .body(body),
        Err(e) => {
            error!(error=%e, "Failed to render template");
            HttpResponse::InternalServerError().finish()
        }
    }
}

#[get("/sync")]
pub(super) async fn sync_list(
    ThinData(flick_sync): ThinData<FlickSync>,
    status: Data<Mutex<SyncStatus>>,
    HxTarget(target): HxTarget,
) -> HttpResponse {
    let sidebar = if target.is_some() {
        None
    } else {
        Some(Sidebar::build(&flick_sync, &status).await)
    };

    #[derive(Template)]
    #[template(path = "sync.html")]
    struct SyncTemplate {
        sidebar: Option<Sidebar>,
        log: Vec<SyncLogTemplate>,
        progress_bars: Vec<ProgressBarTemplate>,
    }

    let mut template = SyncTemplate {
        sidebar,
        log: Vec::new(),
        progress_bars: Vec::new(),
    };

    let (log, progress) = {
        let status = status.lock().unwrap();
        (
            status.log.clone(),
            status
                .progress
                .values()
                .cloned()
                .collect::<Vec<SyncProgressBar>>(),
        )
    };

    for item in log {
        template.log.push(item.template().await);
    }

    for item in progress {
        template.progress_bars.push(item.template().await);
    }

    render(template)
}

#[get("/resources/{path:.*}")]
pub(super) async fn resources(path: Path<String>) -> HttpResponse {
    let Some(file) = Resources::get(&format!("{path}")) else {
        return HttpResponse::NotFound().finish();
    };

    let mime = match path.rsplit_once('.') {
        Some((_, "js")) => mime::APPLICATION_JAVASCRIPT,
        Some((_, "css")) => mime::TEXT_CSS,
        Some((_, "svg")) => mime::IMAGE_SVG,
        _ => mime::APPLICATION_OCTET_STREAM,
    };

    HttpResponse::Ok()
        .append_header(header::ContentLength(file.data.len()))
        .append_header(header::ContentType(mime))
        .body(SizedStream::new(
            file.data.len() as u64,
            EmbeddedFileStream::new(file),
        ))
}

#[get("/thumbnail/{server}/{type}/{id}")]
pub(super) async fn thumbnail(
    ThinData(flick_sync): ThinData<FlickSync>,
    path: Path<(String, String, String)>,
) -> HttpResponse {
    let (server_id, item_type, item_id) = path.into_inner();

    let Some(server) = flick_sync.server(&server_id).await else {
        return HttpResponse::NotFound().finish();
    };

    let file = match item_type.as_str() {
        "video" => {
            let Some(item) = server.video(&item_id).await else {
                return HttpResponse::NotFound().finish();
            };

            item.thumbnail().await
        }
        "show" => {
            let Some(item) = server.show(&item_id).await else {
                return HttpResponse::NotFound().finish();
            };

            item.thumbnail().await
        }
        "playlist" => {
            let Some(item) = server.playlist(&item_id).await else {
                return HttpResponse::NotFound().finish();
            };

            item.thumbnail().await
        }
        "collection" => {
            let Some(item) = server.collection(&item_id).await else {
                return HttpResponse::NotFound().finish();
            };

            item.thumbnail().await
        }
        _ => return HttpResponse::NotFound().finish(),
    };

    let Ok(Some(file)) = file else {
        return HttpResponse::NotFound().finish();
    };

    let size = file.len().await;

    let Ok(reader) = file.async_read().await else {
        return HttpResponse::NotFound().finish();
    };

    let mut response = HttpResponse::Ok();

    response.append_header(header::ContentType(mime::IMAGE_JPEG));

    if let Ok(size) = size {
        response.append_header(header::ContentLength(size as usize));
        response.body(SizedStream::new(
            size,
            ReaderStream::new(BufReader::new(reader)),
        ))
    } else {
        response.streaming(ReaderStream::new(BufReader::new(reader)))
    }
}

#[get("/stream/{server}/{video_id}/{part}")]
pub(super) async fn video_stream(
    ThinData(flick_sync): ThinData<FlickSync>,
    req: HttpRequest,
    path: Path<(String, String, usize)>,
) -> HttpResponse {
    let (server_id, video_id, part_index) = path.into_inner();

    let Some(server) = flick_sync.server(&server_id).await else {
        return HttpResponse::NotFound().finish();
    };

    let Some(video) = server.video(&video_id).await else {
        return HttpResponse::NotFound().finish();
    };

    let parts = video.parts().await;
    let Some(part) = parts.get(part_index) else {
        return HttpResponse::NotFound().finish();
    };

    let Ok(Some(file)) = part.file().await else {
        return HttpResponse::NotFound().finish();
    };

    let Ok(mime_type) = file.mime_type().await else {
        return HttpResponse::NotFound().finish();
    };

    let Ok(size) = file.len().await else {
        return HttpResponse::NotFound().finish();
    };

    let Ok(reader) = file.async_read().await else {
        return HttpResponse::NotFound().finish();
    };
    let mut reader = BufReader::new(reader);

    if let Some(header::Range::Bytes(spec)) = req
        .headers()
        .get(header::RANGE)
        .and_then(|hv| hv.to_str().ok())
        .and_then(|hv| header::Range::from_str(hv).ok())
    {
        if spec.len() == 1 {
            match spec[0] {
                header::ByteRangeSpec::From(start) => {
                    if reader.seek(SeekFrom::Start(start)).await.is_err() {
                        return HttpResponse::NotFound().finish();
                    }

                    HttpResponse::PartialContent()
                        .append_header(header::ContentRange(header::ContentRangeSpec::Bytes {
                            range: Some((start, size - 1)),
                            instance_length: Some(size),
                        }))
                        .append_header(header::ContentType(mime_type))
                        .append_header((header::ACCEPT_RANGES, "bytes"))
                        .body(SizedStream::new(size - start, ReaderStream::new(reader)))
                }
                header::ByteRangeSpec::Last(end) => {
                    if reader.seek(SeekFrom::End(-(end as i64))).await.is_err() {
                        return HttpResponse::NotFound().finish();
                    }

                    HttpResponse::PartialContent()
                        .append_header(header::ContentRange(header::ContentRangeSpec::Bytes {
                            range: Some((size - end, end)),
                            instance_length: Some(size),
                        }))
                        .append_header(header::ContentType(mime_type))
                        .append_header((header::ACCEPT_RANGES, "bytes"))
                        .body(SizedStream::new(end, ReaderStream::new(reader)))
                }
                header::ByteRangeSpec::FromTo(start, end) => {
                    if reader.seek(SeekFrom::Start(start)).await.is_err() {
                        return HttpResponse::NotFound().finish();
                    }

                    HttpResponse::PartialContent()
                        .append_header(header::ContentRange(header::ContentRangeSpec::Bytes {
                            range: Some((start, end)),
                            instance_length: Some(size),
                        }))
                        .append_header(header::ContentType(mime_type))
                        .append_header((header::ACCEPT_RANGES, "bytes"))
                        .body(SizedStream::new(
                            end - start + 1,
                            StreamLimiter::new(ReaderStream::new(reader), start, end + 1),
                        ))
                }
            }
        } else {
            HttpResponse::Ok()
                .append_header(header::ContentType(mime_type))
                .append_header((header::ACCEPT_RANGES, "bytes"))
                .body(SizedStream::new(size, ReaderStream::new(reader)))
        }
    } else {
        HttpResponse::Ok()
            .append_header(header::ContentType(mime_type))
            .append_header((header::ACCEPT_RANGES, "bytes"))
            .body(SizedStream::new(size, ReaderStream::new(reader)))
    }
}

#[derive(Debug)]
struct SidebarLibrary {
    id: String,
    server: String,
    title: String,
    library_type: LibraryType,
}

#[derive(Debug)]
struct SidebarPlaylist {
    id: String,
    server: String,
    title: String,
}

#[derive(Debug)]
struct Sidebar {
    is_syncing: bool,
    libraries: Vec<SidebarLibrary>,
    playlists: Vec<SidebarPlaylist>,
}

impl Sidebar {
    async fn build(flick_sync: &FlickSync, status: &Mutex<SyncStatus>) -> Self {
        let is_syncing = status.lock().unwrap().is_syncing;

        let mut libraries = Vec::new();
        let mut playlists = Vec::new();

        for server in flick_sync.servers().await {
            for library in server.libraries().await {
                libraries.push(SidebarLibrary {
                    id: library.id().to_owned(),
                    server: server.id().to_owned(),
                    title: library.title().await,
                    library_type: library.library_type(),
                });
            }

            for playlist in server.playlists().await {
                playlists.push(SidebarPlaylist {
                    id: playlist.id().to_owned(),
                    server: server.id().to_owned(),
                    title: playlist.title().await,
                });
            }
        }

        Self {
            is_syncing,
            libraries,
            playlists,
        }
    }
}

struct Thumbnail {
    url: String,
    image: String,
    name: String,
}

impl PartialEq for Thumbnail {
    fn eq(&self, other: &Self) -> bool {
        uniform_title(&self.name) == uniform_title(&other.name)
    }
}

impl Eq for Thumbnail {}

impl Ord for Thumbnail {
    fn cmp(&self, other: &Self) -> Ordering {
        uniform_title(&self.name).cmp(&uniform_title(&other.name))
    }
}

impl PartialOrd for Thumbnail {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Thumbnail {
    async fn from_season(season: Season) -> Self {
        let server = season.server();
        let show = season.show().await;
        let library = show.library().await;

        Self {
            url: format!(
                "/library/{}/{}/season/{}",
                server.id(),
                library.id(),
                season.id()
            ),
            image: format!("/thumbnail/{}/show/{}", server.id(), show.id()),
            name: season.title().await,
        }
    }

    async fn from_show(show: Show) -> Self {
        let server = show.server();
        let library = show.library().await;

        Self {
            url: format!(
                "/library/{}/{}/show/{}",
                server.id(),
                library.id(),
                show.id()
            ),
            image: format!("/thumbnail/{}/show/{}", server.id(), show.id()),
            name: show.title().await,
        }
    }

    async fn from_video(video: Video) -> Self {
        let server = video.server();
        let library = video.library().await;

        Self {
            url: format!(
                "/library/{}/{}/video/{}",
                server.id(),
                library.id(),
                video.id()
            ),
            image: format!("/thumbnail/{}/video/{}", server.id(), video.id()),
            name: video.title().await,
        }
    }

    async fn from_collection(collection: Collection) -> Self {
        let server = collection.server();
        let library = collection.library().await;

        Self {
            url: format!(
                "/library/{}/{}/collection/{}",
                server.id(),
                library.id(),
                collection.id()
            ),
            image: format!("/thumbnail/{}/collection/{}", server.id(), collection.id()),
            name: collection.title().await,
        }
    }
}

#[derive(Template)]
#[template(path = "library.html")]
struct LibraryTemplate<'a> {
    sidebar: Option<Sidebar>,
    title: String,
    has_collections: bool,
    browse_url: &'a str,
    collection_url: &'a str,
    items: Vec<Thumbnail>,
}

#[derive(Template)]
#[template(path = "list.html")]
struct ListTemplate {
    sidebar: Option<Sidebar>,
    title: String,
    items: Vec<Thumbnail>,
}

#[get("/library/{server}/{id}")]
pub(super) async fn library_contents(
    ThinData(flick_sync): ThinData<FlickSync>,
    status: Data<Mutex<SyncStatus>>,
    HxTarget(target): HxTarget,
    path: Path<(String, String)>,
) -> HttpResponse {
    let (server_id, library_id) = path.into_inner();

    let Some(server) = flick_sync.server(&server_id).await else {
        return HttpResponse::NotFound().finish();
    };

    let Some(library) = server.library(&library_id).await else {
        return HttpResponse::NotFound().finish();
    };

    let sidebar = if target.is_some() {
        None
    } else {
        Some(Sidebar::build(&flick_sync, &status).await)
    };

    let browse_url = format!("/library/{}/{}", server.id(), library.id());
    let collection_url = format!("{browse_url}/collections");

    match library {
        Library::Movie(lib) => {
            let mut thumbs = Vec::new();
            for movie in lib.movies().await {
                if movie.is_downloaded().await {
                    thumbs.push(Thumbnail::from_video(Video::Movie(movie)).await);
                }
            }
            thumbs.sort();

            let template = LibraryTemplate {
                sidebar,
                title: lib.title().await,
                has_collections: !lib.collections().await.is_empty(),
                browse_url: &browse_url,
                collection_url: &collection_url,
                items: thumbs,
            };

            render(template)
        }
        Library::Show(lib) => {
            let mut thumbs = Vec::new();
            for show in lib.shows().await {
                thumbs.push(Thumbnail::from_show(show).await);
            }
            thumbs.sort();

            let template = LibraryTemplate {
                sidebar,
                title: lib.title().await,
                has_collections: !lib.collections().await.is_empty(),
                browse_url: &browse_url,
                collection_url: &collection_url,
                items: thumbs,
            };

            render(template)
        }
    }
}

#[get("/library/{server}/{id}/collections")]
pub(super) async fn library_collections(
    ThinData(flick_sync): ThinData<FlickSync>,
    status: Data<Mutex<SyncStatus>>,
    HxTarget(target): HxTarget,
    path: Path<(String, String)>,
) -> HttpResponse {
    let (server_id, library_id) = path.into_inner();

    let Some(server) = flick_sync.server(&server_id).await else {
        return HttpResponse::NotFound().finish();
    };

    let Some(library) = server.library(&library_id).await else {
        return HttpResponse::NotFound().finish();
    };

    let sidebar = if target.is_some() {
        None
    } else {
        Some(Sidebar::build(&flick_sync, &status).await)
    };

    let browse_url = format!("/library/{}/{}", server.id(), library.id());
    let collection_url = format!("{browse_url}/collections");

    let mut thumbs = Vec::new();
    for collection in library.collections().await {
        thumbs.push(Thumbnail::from_collection(collection).await);
    }
    thumbs.sort();

    let template = LibraryTemplate {
        sidebar,
        title: library.title().await,
        has_collections: true,
        browse_url: &browse_url,
        collection_url: &collection_url,
        items: thumbs,
    };

    render(template)
}

#[get("/library/{server}/{library_id}/collection/{collection_id}")]
pub(super) async fn collection_contents(
    ThinData(flick_sync): ThinData<FlickSync>,
    status: Data<Mutex<SyncStatus>>,
    HxTarget(target): HxTarget,
    path: Path<(String, String, String)>,
) -> HttpResponse {
    let (server_id, _, collection_id) = path.into_inner();

    let Some(server) = flick_sync.server(&server_id).await else {
        return HttpResponse::NotFound().finish();
    };

    let Some(collection) = server.collection(&collection_id).await else {
        return HttpResponse::NotFound().finish();
    };

    let sidebar = if target.is_some() {
        None
    } else {
        Some(Sidebar::build(&flick_sync, &status).await)
    };

    let mut thumbs = Vec::new();
    match collection {
        Collection::Movie(ref c) => {
            for movie in c.movies().await {
                if movie.is_downloaded().await {
                    thumbs.push(Thumbnail::from_video(Video::Movie(movie)).await);
                }
            }
        }
        Collection::Show(ref c) => {
            for show in c.shows().await {
                thumbs.push(Thumbnail::from_show(show).await);
            }
        }
    };

    thumbs.sort();

    let template = ListTemplate {
        sidebar,
        title: collection.title().await,
        items: thumbs,
    };

    render(template)
}

#[get("/library/{server}/{library_id}/show/{collection_id}")]
pub(super) async fn show_contents(
    ThinData(flick_sync): ThinData<FlickSync>,
    status: Data<Mutex<SyncStatus>>,
    HxTarget(target): HxTarget,
    path: Path<(String, String, String)>,
) -> HttpResponse {
    let (server_id, _, show_id) = path.into_inner();

    let Some(server) = flick_sync.server(&server_id).await else {
        return HttpResponse::NotFound().finish();
    };

    let Some(show) = server.show(&show_id).await else {
        return HttpResponse::NotFound().finish();
    };

    let sidebar = if target.is_some() {
        None
    } else {
        Some(Sidebar::build(&flick_sync, &status).await)
    };

    let mut thumbs = Vec::new();
    for season in show.seasons().await {
        thumbs.push(Thumbnail::from_season(season).await);
    }

    let template = ListTemplate {
        sidebar,
        title: show.title().await,
        items: thumbs,
    };

    render(template)
}

#[get("/library/{server}/{library_id}/season/{collection_id}")]
pub(super) async fn season_contents(
    ThinData(flick_sync): ThinData<FlickSync>,
    status: Data<Mutex<SyncStatus>>,
    HxTarget(target): HxTarget,
    path: Path<(String, String, String)>,
) -> HttpResponse {
    let (server_id, _, season_id) = path.into_inner();

    let Some(server) = flick_sync.server(&server_id).await else {
        return HttpResponse::NotFound().finish();
    };

    let Some(season) = server.season(&season_id).await else {
        return HttpResponse::NotFound().finish();
    };

    let sidebar = if target.is_some() {
        None
    } else {
        Some(Sidebar::build(&flick_sync, &status).await)
    };

    let mut thumbs = Vec::new();
    for episode in season.episodes().await {
        if episode.is_downloaded().await {
            thumbs.push(Thumbnail::from_video(Video::Episode(episode)).await);
        }
    }

    let template = ListTemplate {
        sidebar,
        title: format!(
            "{} - {}",
            season.show().await.title().await,
            season.title().await
        ),
        items: thumbs,
    };

    render(template)
}

#[get("/playlist/{server}/{id}")]
pub(super) async fn playlist_contents(
    ThinData(flick_sync): ThinData<FlickSync>,
    status: Data<Mutex<SyncStatus>>,
    HxTarget(target): HxTarget,
    path: Path<(String, String)>,
) -> HttpResponse {
    let (server_id, playlist_id) = path.into_inner();

    let Some(server) = flick_sync.server(&server_id).await else {
        return HttpResponse::NotFound().finish();
    };

    let Some(playlist) = server.playlist(&playlist_id).await else {
        return HttpResponse::NotFound().finish();
    };

    let sidebar = if target.is_some() {
        None
    } else {
        Some(Sidebar::build(&flick_sync, &status).await)
    };

    #[derive(Template)]
    #[template(path = "playlist.html")]
    struct Playlist {
        sidebar: Option<Sidebar>,
        title: String,
        items: Vec<Thumbnail>,
    }

    let mut items = Vec::new();
    for video in playlist.videos().await {
        if video.is_downloaded().await {
            items.push(Thumbnail::from_video(video).await);
        }
    }

    let template = Playlist {
        sidebar,
        title: playlist.title().await,
        items,
    };

    render(template)
}

#[derive(Debug, Deserialize)]
pub struct PlaybackPosition {
    position: f64,
}

#[post("/playback/{server}/{library_id}/video/{video_id}")]
pub(super) async fn update_playback_position(
    ThinData(flick_sync): ThinData<FlickSync>,
    path: Path<(String, String, String)>,
    query: Query<PlaybackPosition>,
) -> HttpResponse {
    let (server_id, _, video_id) = path.into_inner();

    let Some(server) = flick_sync.server(&server_id).await else {
        return HttpResponse::NotFound().finish();
    };

    let Some(video) = server.video(&video_id).await else {
        return HttpResponse::NotFound().finish();
    };

    let position = (query.position * 1000.0).round() as u64;
    let _ = video.set_playback_position(position).await;

    HttpResponse::Ok().finish()
}

#[get("/library/{server}/{library_id}/video/{video_id}")]
pub(super) async fn video_page(
    ThinData(flick_sync): ThinData<FlickSync>,
    status: Data<Mutex<SyncStatus>>,
    HxTarget(target): HxTarget,
    req: HttpRequest,
    path: Path<(String, String, String)>,
) -> HttpResponse {
    let (server_id, _, video_id) = path.into_inner();

    let Some(server) = flick_sync.server(&server_id).await else {
        return HttpResponse::NotFound().finish();
    };

    let Some(video) = server.video(&video_id).await else {
        return HttpResponse::NotFound().finish();
    };

    let sidebar = if target.is_some() {
        None
    } else {
        Some(Sidebar::build(&flick_sync, &status).await)
    };

    let url_base = if let Some(conn_info) = req.conn_data::<ConnectionInfo>() {
        if conn_info.local_addr.port() == 443 {
            format!("http://{}/", conn_info.local_addr.ip())
        } else {
            format!("http://{}/", conn_info.local_addr)
        }
    } else {
        "".to_string()
    };

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct VideoPart {
        url: String,
        mime_type: String,
        duration: f64,
    }

    let mut parts = Vec::new();
    for part in video.parts().await {
        let Ok(Some(file)) = part.file().await else {
            return HttpResponse::NotFound().finish();
        };

        let Ok(mime_type) = file.mime_type().await else {
            return HttpResponse::NotFound().finish();
        };

        parts.push(VideoPart {
            url: format!(
                "{url_base}stream/{server_id}/{}/{}",
                video.id(),
                part.index()
            ),
            mime_type: mime_type.to_string(),
            duration: part.duration().await.as_millis() as f64 / 1000.0,
        })
    }

    #[derive(Template)]
    #[template(path = "video.html")]
    struct Video {
        sidebar: Option<Sidebar>,
        title: String,
        parts: Vec<VideoPart>,
        playback_position: f64,
    }

    let playback_state = video.playback_state().await;
    let playback_position = match playback_state {
        PlaybackState::Unplayed | PlaybackState::Played => 0.0,
        PlaybackState::InProgress { position } => position as f64 / 1000.0,
    };

    let template = Video {
        sidebar,
        title: video.title().await,
        parts,
        playback_position,
    };

    render(template)
}

#[get("/events")]
pub(super) async fn events(ThinData(event_sender): ThinData<Sender<Event>>) -> HttpResponse {
    let receiver = event_sender.subscribe();

    let event_stream = BroadcastStream::new(receiver)
        .try_filter_map(async |event| Ok(event.to_string().await))
        .map_ok(Bytes::from_owner);

    HttpResponse::Ok()
        .append_header(header::ContentType(mime::TEXT_EVENT_STREAM))
        .streaming(event_stream)
}

#[get("/")]
pub(super) async fn index_page(
    ThinData(flick_sync): ThinData<FlickSync>,
    status: Data<Mutex<SyncStatus>>,
    HxTarget(target): HxTarget,
) -> HttpResponse {
    let sidebar = if target.is_some() {
        None
    } else {
        Some(Sidebar::build(&flick_sync, &status).await)
    };

    #[derive(Template, Debug)]
    #[template(path = "index.html")]
    struct Index {
        sidebar: Option<Sidebar>,
    }

    let template = Index { sidebar };

    render(template)
}
