use std::future::{Ready, ready};

use actix_web::{
    FromRequest, HttpResponse, get,
    http::header,
    web::{Path, ThinData},
};
use flick_sync::{FlickSync, LibraryType};
use rinja::Template;

use crate::{EmbeddedFileStream, Resources, error::Error};

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
        _ => mime::APPLICATION_OCTET_STREAM,
    };

    Ok(HttpResponse::Ok()
        .append_header(header::ContentLength(file.data.len()))
        .append_header(header::ContentType(mime))
        .streaming(EmbeddedFileStream::new(file)))
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

    #[derive(Template)]
    #[template(path = "library.html")]
    struct Library {
        sidebar: Option<Sidebar>,
        title: String,
    }

    let template = Library {
        sidebar,
        title: library.title().await,
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
    }

    let template = Playlist {
        sidebar,
        title: playlist.title().await,
    };

    render(template)
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
