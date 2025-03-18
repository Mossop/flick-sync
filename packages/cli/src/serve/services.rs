use std::{
    cmp::Ordering,
    future::{Ready, ready},
};

use actix_web::{
    FromRequest, HttpResponse, get,
    http::header,
    web::{Path, ThinData},
};
use bytes::Bytes;
use flick_sync::{Collection, FlickSync, Library, LibraryType, Show, Video};
use futures::TryStreamExt;
use rinja::Template;
use tokio::{io::BufReader, sync::broadcast::Sender};
use tokio_stream::wrappers::BroadcastStream;
use tokio_util::io::ReaderStream;

use crate::{EmbeddedFileStream, Resources, error::Error, serve::Event};

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

fn render<T: Template>(template: T) -> Result<HttpResponse, Error> {
    Ok(HttpResponse::Ok()
        .append_header(header::ContentType(mime::TEXT_HTML))
        .body(template.render()?))
}

#[get("/resources/{path:.*}")]
pub(super) async fn resources(path: Path<String>) -> Result<HttpResponse, Error> {
    let Some(file) = Resources::get(&format!("{path}")) else {
        return Ok(HttpResponse::NotFound().finish());
    };

    let mime = match path.rsplit_once('.') {
        Some((_, "js")) => mime::APPLICATION_JAVASCRIPT,
        Some((_, "css")) => mime::TEXT_CSS,
        Some((_, "svg")) => mime::IMAGE_SVG,
        _ => mime::APPLICATION_OCTET_STREAM,
    };

    Ok(HttpResponse::Ok()
        .append_header(header::ContentLength(file.data.len()))
        .append_header(header::ContentType(mime))
        .streaming(EmbeddedFileStream::new(file)))
}

#[get("/thumbnail/{server}/{type}/{id}")]
pub(super) async fn thumbnail(
    ThinData(flick_sync): ThinData<FlickSync>,
    path: Path<(String, String, String)>,
) -> Result<HttpResponse, Error> {
    let (server_id, item_type, item_id) = path.into_inner();

    let Some(server) = flick_sync.server(&server_id).await else {
        return Ok(HttpResponse::NotFound().finish());
    };

    let file = match item_type.as_str() {
        "video" => {
            let Some(item) = server.video(&item_id).await else {
                return Ok(HttpResponse::NotFound().finish());
            };

            item.thumbnail().await
        }
        "show" => {
            let Some(item) = server.show(&item_id).await else {
                return Ok(HttpResponse::NotFound().finish());
            };

            item.thumbnail().await
        }
        "playlist" => {
            let Some(item) = server.playlist(&item_id).await else {
                return Ok(HttpResponse::NotFound().finish());
            };

            item.thumbnail().await
        }
        "collection" => {
            let Some(item) = server.collection(&item_id).await else {
                return Ok(HttpResponse::NotFound().finish());
            };

            item.thumbnail().await
        }
        _ => return Ok(HttpResponse::NotFound().finish()),
    };

    let Ok(Some(file)) = file else {
        return Ok(HttpResponse::NotFound().finish());
    };

    let mut response = HttpResponse::Ok();
    if let Ok(size) = file.len().await {
        response.append_header(header::ContentLength(size as usize));
    }

    let Ok(reader) = file.async_read().await else {
        return Ok(HttpResponse::NotFound().finish());
    };

    Ok(response
        .append_header(header::ContentType(mime::IMAGE_JPEG))
        .streaming(ReaderStream::new(BufReader::new(reader))))
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
    libraries: Vec<SidebarLibrary>,
    playlists: Vec<SidebarPlaylist>,
}

impl Sidebar {
    async fn build(flick_sync: &FlickSync) -> Self {
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

fn uniform_title(st: &str) -> String {
    let title = st.to_lowercase();

    title
        .trim()
        .trim_start_matches("a ")
        .trim()
        .trim_start_matches("the ")
        .trim()
        .to_string()
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
    async fn from_show(show: Show) -> Self {
        let server = show.server();
        let library = show.library().await;

        Self {
            url: format!("/library/{}/{}/{}", server.id(), library.id(), show.id()),
            image: format!("/thumbnail/{}/show/{}", server.id(), show.id()),
            name: show.title().await,
        }
    }

    async fn from_video(video: Video) -> Self {
        let server = video.server();
        let library = video.library().await;

        Self {
            url: format!("/library/{}/{}/{}", server.id(), library.id(), video.id()),
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

#[get("/library/{server}/{id}")]
pub(super) async fn library_list(
    ThinData(flick_sync): ThinData<FlickSync>,
    HxTarget(target): HxTarget,
    path: Path<(String, String)>,
) -> Result<HttpResponse, Error> {
    let (server_id, library_id) = path.into_inner();

    let Some(server) = flick_sync.server(&server_id).await else {
        return Ok(HttpResponse::NotFound().finish());
    };

    let Some(library) = server.library(&library_id).await else {
        return Ok(HttpResponse::NotFound().finish());
    };

    let sidebar = if target.is_some() {
        None
    } else {
        Some(Sidebar::build(&flick_sync).await)
    };

    let browse_url = format!("/library/{}/{}", server.id(), library.id());
    let collection_url = format!("{browse_url}/collections");

    match library {
        Library::Movie(lib) => {
            let mut thumbs = Vec::new();
            for movie in lib.movies().await {
                thumbs.push(Thumbnail::from_video(Video::Movie(movie)).await);
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
pub(super) async fn collection_list(
    ThinData(flick_sync): ThinData<FlickSync>,
    HxTarget(target): HxTarget,
    path: Path<(String, String)>,
) -> Result<HttpResponse, Error> {
    let (server_id, library_id) = path.into_inner();

    let Some(server) = flick_sync.server(&server_id).await else {
        return Ok(HttpResponse::NotFound().finish());
    };

    let Some(library) = server.library(&library_id).await else {
        return Ok(HttpResponse::NotFound().finish());
    };

    let sidebar = if target.is_some() {
        None
    } else {
        Some(Sidebar::build(&flick_sync).await)
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

#[get("/playlist/{server}/{id}")]
pub(super) async fn playlist_list(
    ThinData(flick_sync): ThinData<FlickSync>,
    HxTarget(target): HxTarget,
    path: Path<(String, String)>,
) -> Result<HttpResponse, Error> {
    let (server_id, playlist_id) = path.into_inner();

    let Some(server) = flick_sync.server(&server_id).await else {
        return Ok(HttpResponse::NotFound().finish());
    };

    let Some(playlist) = server.playlist(&playlist_id).await else {
        return Ok(HttpResponse::NotFound().finish());
    };

    let sidebar = if target.is_some() {
        None
    } else {
        Some(Sidebar::build(&flick_sync).await)
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
        items.push(Thumbnail::from_video(video).await);
    }

    let template = Playlist {
        sidebar,
        title: playlist.title().await,
        items,
    };

    render(template)
}

#[get("/events")]
pub(super) async fn events(ThinData(event_sender): ThinData<Sender<Event>>) -> HttpResponse {
    let receiver = event_sender.subscribe();

    let event_stream = BroadcastStream::new(receiver)
        .and_then(async |event| Ok(event.to_string().await))
        .map_ok(Bytes::from_owner);

    HttpResponse::Ok()
        .append_header(header::ContentType(mime::TEXT_EVENT_STREAM))
        .streaming(event_stream)
}

#[get("/")]
pub(super) async fn index(
    ThinData(flick_sync): ThinData<FlickSync>,
    HxTarget(target): HxTarget,
) -> Result<HttpResponse, Error> {
    let sidebar = if target.is_some() {
        None
    } else {
        Some(Sidebar::build(&flick_sync).await)
    };

    #[derive(Template, Debug)]
    #[template(path = "index.html")]
    struct Index {
        sidebar: Option<Sidebar>,
    }

    let template = Index { sidebar };

    render(template)
}
