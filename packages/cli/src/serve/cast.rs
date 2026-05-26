use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    sync::Arc,
    time::{Duration, Instant},
};

use actix_web::{
    HttpRequest, HttpResponse, Responder, get,
    web::{Payload, Query, ThinData},
};
use actix_ws::codec::{CodecMessage, JsonCodec};
use askama::Template;
use flick_sync::{FlickSync, PlaybackState, Video};
use oxicast::{
    Capability, CastApp, CastClient, CastEvent, IdleReason, Image, MediaInfo, MediaMetadata,
    MediaStatus, PlayerState, discovery,
};
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{Mutex, broadcast},
    time::sleep,
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, trace, warn};
use uuid::Uuid;

use crate::serve::{ConnectionInfo, Event, ServiceData};

const CONTENT_PREFIX: &str = "urn:flicksync:video:";
const REFRESH_RATE: u64 = 5;
/// The position reported when playback starts can be inaccurate so a fresh
/// status is requested this soon after.
const PLAYING_REFRESH_RATE: u64 = 1;
const PLAYBACK_UPDATE_RATE: u64 = 5;
const CONNECT_TIMEOUT: u64 = 10;

#[derive(Template, Clone)]
#[template(path = "castdevices.html")]
pub(super) struct AvailableDevices {
    devices: Vec<(String, bool, String)>,
}

impl AvailableDevices {
    async fn from_devices(source: &Vec<&CastDevice>) -> Self {
        let mut devices = Vec::new();

        for device in source {
            devices.push((
                device.id.clone(),
                device.state.lock().await.is_some(),
                device.name.clone(),
            ));
        }

        Self { devices }
    }
}

#[derive(Template, Clone)]
#[template(path = "castsessions.html")]
pub(super) struct CastSessions {
    media: Vec<(String, String)>,
}

impl CastSessions {
    async fn from_devices(source: &Vec<&CastDevice>) -> Self {
        let mut media = Vec::new();

        for device in source {
            if let Some(ref state) = *device.state.lock().await {
                let url = format!(
                    "/library/{}/{}/video/{}?cast={}",
                    state.video.server().id(),
                    state.video.library().await.id(),
                    state.video.id(),
                    device.id.clone()
                );
                media.push((url, state.video.title().await.clone()));
            }
        }

        Self { media }
    }
}

#[derive(Clone)]
struct CastState {
    video: Video,
    last_update: Instant,
    status: MediaStatus,
}

impl CastState {
    fn estimate_position(&self) -> f64 {
        if self.status.player_state == PlayerState::Playing {
            self.status.current_time + (Instant::now() - self.last_update).as_secs_f64()
        } else {
            self.status.current_time
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum PlayState {
    Idle,
    Buffering,
    Playing,
    Paused,
}

impl From<PlayerState> for PlayState {
    fn from(state: PlayerState) -> Self {
        match state {
            PlayerState::Buffering => PlayState::Buffering,
            PlayerState::Idle => PlayState::Idle,
            PlayerState::Playing => PlayState::Playing,
            PlayerState::Paused => PlayState::Paused,
            _ => PlayState::Idle,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum CastStateEvent {
    CastState {
        server: String,
        video: String,
        state: PlayState,
        position: f64,
    },
    Stopped,
    Disconnected,
    Error {
        message: String,
    },
}

impl From<Option<&CastState>> for CastStateEvent {
    fn from(value: Option<&CastState>) -> Self {
        let Some(state) = value else {
            return CastStateEvent::Stopped;
        };

        CastStateEvent::CastState {
            server: state.video.server().id().to_string(),
            video: state.video.id().to_string(),
            state: PlayState::from(state.status.player_state),
            position: state.estimate_position(),
        }
    }
}

fn content_id_from_video(video: &Video) -> String {
    format!("{CONTENT_PREFIX}{}:{}", video.server().id(), video.id())
}

async fn video_from_content_id(flick_sync: &FlickSync, content_id: &str) -> Option<Video> {
    let mut params = content_id.strip_prefix(CONTENT_PREFIX)?.split(':');

    let server = flick_sync.server(params.next()?).await?;
    server.video(params.next()?).await
}

/// Finds the local address used to reach the given remote address. Nothing is
/// sent, connecting a UDP socket just selects the route.
fn local_addr_for(remote: IpAddr) -> Option<IpAddr> {
    let bind: SocketAddr = match remote {
        IpAddr::V4(_) => (Ipv4Addr::UNSPECIFIED, 0).into(),
        IpAddr::V6(_) => (Ipv6Addr::UNSPECIFIED, 0).into(),
    };

    let socket = UdpSocket::bind(bind).ok()?;
    socket.connect((remote, 9)).ok()?;
    let local = socket.local_addr().ok()?.ip();

    (!local.is_unspecified()).then_some(local)
}

#[derive(Clone)]
struct CastDevice {
    id: String,
    ip: IpAddr,
    name: String,
    state: Arc<Mutex<Option<CastState>>>,
    client: CastClient,
    sender: broadcast::Sender<CastStateEvent>,
}

impl CastDevice {
    async fn last_event(&self) -> CastStateEvent {
        let state = self.state.lock().await;
        CastStateEvent::from(state.as_ref())
    }

    fn subscribe(&self) -> broadcast::Receiver<CastStateEvent> {
        self.sender.subscribe()
    }

    /// Tells connected clients that casting failed.
    fn report_error(&self, message: impl Into<String>) {
        let _ = self.sender.send(CastStateEvent::Error {
            message: message.into(),
        });
    }

    /// The base URL that the cast device can reach this server on.
    fn url_base(&self, http_port: u16) -> Option<String> {
        let local = local_addr_for(self.ip)?;
        Some(format!("http://{}", SocketAddr::new(local, http_port)))
    }

    async fn load(&self, url_base: &str, video: Video, start_position: f64) {
        let mime_type = match video.file().await {
            Ok(Some(file)) => match file.mime_type().await {
                Ok(m) => m,
                Err(e) => {
                    warn!(ip = %self.ip, video=video.id(), error = ?e, "Unable to cast media, failed to determine mimetype");
                    self.report_error(
                        "Unable to cast the video, its type could not be determined.",
                    );
                    return;
                }
            },
            Ok(None) => {
                warn!(ip = %self.ip, video=video.id(), "Unable to cast media, file missing");
                self.report_error("Unable to cast the video, it has not been downloaded.");
                return;
            }
            Err(_) => {
                warn!(ip = %self.ip, video=video.id(), "Unable to cast media, timed out acquiring file lock");
                self.report_error("Unable to cast the video, it is currently in use.");
                return;
            }
        };

        let metadata = match &video {
            Video::Episode(episode) => {
                let show = episode.show().await;
                let season = episode.season().await;
                MediaMetadata::TvShow {
                    series_title: Some(show.title().await),
                    episode_title: Some(episode.title().await),
                    season: Some(season.index().await as u32),
                    episode: episode.index().await.map(|i| i as u32),
                    images: vec![Image {
                        url: format!(
                            "{url_base}/thumbnail/{}/show/{}",
                            episode.server().id(),
                            show.id()
                        ),
                        width: None,
                        height: None,
                    }],
                }
            }
            Video::Movie(movie) => MediaMetadata::Movie {
                title: Some(movie.title().await),
                subtitle: None,
                studio: None,
                images: vec![Image {
                    url: format!(
                        "{url_base}/thumbnail/{}/video/{}",
                        movie.server().id(),
                        movie.id()
                    ),
                    width: None,
                    height: None,
                }],
            },
        };

        let mut media =
            MediaInfo::new(content_id_from_video(&video), mime_type.to_string()).metadata(metadata);

        media.content_url = Some(format!(
            "{url_base}/stream/{}/{}",
            video.server().id(),
            video.id()
        ));

        debug!(ip = %self.ip, video=video.id(), url = media.content_url, "Casting media");

        if let Err(e) = self.client.launch_app(&CastApp::DefaultMediaReceiver).await {
            warn!(ip = %self.ip, video=video.id(), error = ?e, "Unable to cast media, failed to launch media receiver");
            self.report_error(
                "Unable to cast the video, the cast device could not start its media player.",
            );
            return;
        }

        // A load failure reported later by the device arrives as a LoadFailed
        // event, handled in monitor.
        if let Err(e) = self
            .client
            .load_media(&media, true, start_position, None)
            .await
        {
            warn!(ip = %self.ip, video=video.id(), error = ?e, "Unable to cast media, failed to load media");
            self.report_error(format!("The cast device was unable to load the video: {e}"));
        }
    }

    async fn play(&self) {
        if let Err(e) = self.client.play().await {
            error!(ip = %self.ip, error = ?e, "Failed to send play request to cast device");
        }
    }

    async fn pause(&self) {
        if let Err(e) = self.client.pause().await {
            error!(ip = %self.ip, error = ?e, "Failed to send pause request to cast device");
        }
    }

    async fn seek(&self, position: f64) {
        if let Err(e) = self.client.seek(position).await {
            error!(ip = %self.ip, error = ?e, "Failed to send seek request to cast device");
        }
    }

    async fn stop(&self) {
        self.update_playback_state().await;

        if let Err(e) = self.client.stop_media().await {
            error!(ip = %self.ip, error = ?e, "Failed to send seek request to cast device");
        }
    }

    async fn update_playback_state(&self) {
        if let Some(state) = self.state.lock().await.as_ref() {
            let position = state.estimate_position();
            let _ = state
                .video
                .set_playback_position((position * 1000.0).round() as u64)
                .await;
        }
    }

    async fn update_state(&self, flick_sync: &FlickSync, status: Option<MediaStatus>) {
        // An interrupted session means the device is switching to new media,
        // the status for that will follow. Applying this would mark the old
        // media as idle and reset its playback position.
        if let Some(status) = &status
            && status.player_state == PlayerState::Idle
            && status.idle_reason == Some(IdleReason::Interrupted)
        {
            trace!(ip=%self.ip, "Ignoring interrupted media status");
            return;
        }

        {
            let mut state = self.state.lock().await;

            if let Some(status) = status {
                if let Some(media) = &status.media {
                    if let Some(video) = video_from_content_id(flick_sync, &media.content_id).await
                    {
                        *state = Some(CastState {
                            video,
                            last_update: Instant::now(),
                            status,
                        });
                        let _ = self.sender.send(CastStateEvent::from(state.as_ref()));
                    } else if state.is_some() {
                        *state = None;
                        let _ = self.sender.send(CastStateEvent::from(state.as_ref()));
                    }
                } else if let Some(ref mut cast_state) = *state {
                    cast_state.status = status;
                    cast_state.last_update = Instant::now();
                    let _ = self.sender.send(CastStateEvent::from(state.as_ref()));
                }
            } else if !state.is_none() {
                *state = None;
                let _ = self.sender.send(CastStateEvent::from(state.as_ref()));
            }
        }

        self.update_playback_state().await;
    }

    async fn monitor(
        self,
        flick_sync: FlickSync,
        devices: CastDevices,
        event_sender: broadcast::Sender<Event>,
    ) {
        if let Ok(receiver_status) = self.client.receiver_status().await
            && let Some(running_app) = receiver_status
                .applications
                .into_iter()
                .find(|a| a.app_id == CastApp::DefaultMediaReceiver.app_id())
        {
            match self.client.join_session(&running_app).await {
                Ok(_) => trace!(ip=%self.ip, "Joined existing session"),
                Err(e) => warn!(ip=%self.ip, error = ?e, "Failed to join existing session"),
            }
        }

        if let Ok(media_status) = self.client.media_status().await {
            trace!(ip=%self.ip, ?media_status, "Initial media status");
            self.update_state(&flick_sync, media_status).await;
        }

        CastManager::send_devices_status(&event_sender, devices.lock().await.values().collect())
            .await;

        let mut playback_update_timeout =
            Box::pin(sleep(Duration::from_secs(PLAYBACK_UPDATE_RATE)));
        let mut refresh_timeout = Box::pin(sleep(Duration::from_secs(REFRESH_RATE)));
        let mut playing_refresh_pending = false;

        loop {
            tokio::select! {
                cast_event = self.client.next_event() => {
                    let Some(event) = cast_event else {
                        break;
                    };

                    trace!(ip=%self.ip, ?event, "Cast event");

                    match event {
                        CastEvent::MediaStatusChanged(status) => {
                            let was_playing = self
                                .state
                                .lock()
                                .await
                                .as_ref()
                                .is_some_and(|s| s.status.player_state == PlayerState::Playing);
                            let started_playing =
                                !was_playing && status.player_state == PlayerState::Playing;

                            self.update_state(&flick_sync, Some(status)).await;
                            CastManager::send_devices_status(
                                &event_sender,
                                devices.lock().await.values().collect(),
                            )
                            .await;

                            // Don't let further status updates delay a pending
                            // refresh after playback started.
                            if started_playing {
                                playing_refresh_pending = true;
                                refresh_timeout =
                                    Box::pin(sleep(Duration::from_secs(PLAYING_REFRESH_RATE)));
                            } else if !playing_refresh_pending {
                                refresh_timeout =
                                    Box::pin(sleep(Duration::from_secs(REFRESH_RATE)));
                            }
                        }
                        CastEvent::MediaSessionEnded { idle_reason, .. }
                            if idle_reason != IdleReason::Interrupted =>
                        {
                            if idle_reason == IdleReason::Finished && let Some(state) = self.state.lock().await.as_ref() {
                                let _ = state.video.set_playback_state(PlaybackState::Played).await;
                            }

                            self.update_state(&flick_sync, None).await;
                            CastManager::send_devices_status(
                                &event_sender,
                                devices.lock().await.values().collect(),
                            )
                            .await;
                            refresh_timeout = Box::pin(sleep(Duration::from_secs(REFRESH_RATE)));
                        }
                        CastEvent::LoadFailed { detailed_error_code, .. } => {
                            warn!(ip=%self.ip, ?detailed_error_code, "Cast device failed to load media");
                            let code = detailed_error_code
                                .map(|code| format!(" (error {code})"))
                                .unwrap_or_default();
                            self.report_error(format!(
                                "The cast device was unable to load the video{code}. Check that it can connect to this server."
                            ));

                            self.update_state(&flick_sync, None).await;
                            CastManager::send_devices_status(
                                &event_sender,
                                devices.lock().await.values().collect(),
                            )
                            .await;
                        }
                        CastEvent::HeartbeatTimeout => {
                            warn!(ip=%self.ip, "Cast device stopped responding");
                            break;
                        }
                        CastEvent::Disconnected(reason) => {
                            warn!(ip=%self.ip, ?reason, "Cast device disconnected");
                            break;
                        }
                        _ => {}
                    }
                }
                _ = &mut playback_update_timeout => {
                    playback_update_timeout = Box::pin(sleep(Duration::from_secs(PLAYBACK_UPDATE_RATE)));

                    self.update_playback_state().await;
                }
                _ = &mut refresh_timeout => {
                    refresh_timeout = Box::pin(sleep(Duration::from_secs(REFRESH_RATE)));
                    playing_refresh_pending = false;

                    // Some failures (e.g. write errors) don't generate an event.
                    if !self.client.is_connected() {
                        warn!(ip=%self.ip, "Lost connection to cast device");
                        break;
                    }

                    // This forces a status update to be delivered in the normal event stream.
                    let _ = self.client.media_status().await;
                }
            }
        }

        self.update_state(&flick_sync, None).await;
        let _ = self.sender.send(CastStateEvent::Disconnected);
        let mut devices = devices.lock().await;
        devices.remove(&self.ip);
        CastManager::send_devices_status(&event_sender, devices.values().collect()).await;
    }
}

type CastDevices = Arc<Mutex<HashMap<IpAddr, CastDevice>>>;

#[derive(Clone)]
pub(super) struct CastManager {
    devices: CastDevices,
    cancellation: CancellationToken,
}

impl CastManager {
    pub(super) fn new(flick_sync: FlickSync, event_sender: broadcast::Sender<Event>) -> Self {
        let devices = CastDevices::default();
        let token = CancellationToken::new();

        tokio::spawn(token.clone().run_until_cancelled_owned(Self::discover(
            flick_sync,
            devices.clone(),
            token.clone(),
            event_sender,
        )));

        Self {
            devices,
            cancellation: token,
        }
    }

    async fn device(&self, device_id: &str) -> Option<CastDevice> {
        self.devices.lock().await.values().find_map(|d| {
            if d.id == device_id {
                Some(d.clone())
            } else {
                None
            }
        })
    }

    pub(super) async fn available_devices(&self) -> AvailableDevices {
        let devices = self.devices.lock().await;
        let mut devices: Vec<&CastDevice> = devices.values().collect();
        devices.sort_by_key(|d| d.name.clone());
        AvailableDevices::from_devices(&devices).await
    }

    pub(super) async fn cast_sessions(&self) -> CastSessions {
        let devices = self.devices.lock().await;
        let mut devices: Vec<&CastDevice> = devices.values().collect();
        devices.sort_by_key(|d| d.name.clone());
        CastSessions::from_devices(&devices).await
    }

    async fn send_devices_status(
        event_sender: &broadcast::Sender<Event>,
        mut devices: Vec<&CastDevice>,
    ) {
        devices.sort_by_key(|d| d.name.clone());

        let _ = event_sender.send(Event::CastingMedia(
            CastSessions::from_devices(&devices).await,
        ));
        let _ = event_sender.send(Event::CastDevices(
            AvailableDevices::from_devices(&devices).await,
        ));
    }

    pub(super) fn destroy(self) {
        self.cancellation.cancel();
    }

    async fn discover(
        flick_sync: FlickSync,
        devices: CastDevices,
        token: CancellationToken,
        event_sender: broadcast::Sender<Event>,
    ) {
        loop {
            let mut stream = match discovery::discover(Duration::from_secs(5)) {
                Ok(s) => s,
                Err(e) => {
                    error!(error=?e, "Error discovering cast devices");
                    sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };

            while let Some(device) = stream.recv().await {
                if devices.lock().await.contains_key(&device.ip) {
                    continue;
                }

                if !device.capabilities.contains(&Capability::VideoOutput) {
                    continue;
                }

                debug!(name = device.name, ip = %device.ip, capabilities=?device.capabilities, "Found cast device");

                // Auto-reconnect is disabled as it takes over a minute to give
                // up on a device that has gone away. Instead, when the
                // connection is lost the device is dropped straight away and
                // will be picked up again by discovery once it is back.
                // The devices lock must not be held while connecting as that
                // would block everything else until the connection completes.
                let connect = CastClient::builder(device.ip.to_string(), device.port)
                    .auto_reconnect(false)
                    .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT))
                    .connect();

                match connect.await {
                    Ok(client) => {
                        let (sender, _) = broadcast::channel(5);
                        let cast_device = CastDevice {
                            id: Uuid::new_v4().to_string(),
                            ip: device.ip,
                            name: device.name,
                            state: Arc::default(),
                            client,
                            sender,
                        };

                        devices.lock().await.insert(device.ip, cast_device.clone());

                        tokio::spawn(token.clone().run_until_cancelled_owned(cast_device.monitor(
                            flick_sync.clone(),
                            devices.clone(),
                            event_sender.clone(),
                        )));
                    }
                    Err(e) => {
                        warn!(ip=%device.ip, error=?e, "Failed to connect to cast device");
                    }
                }
            }

            sleep(Duration::from_secs(5)).await;
        }
    }
}

#[derive(Deserialize)]
struct CastParams {
    device: String,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum CastRequest {
    StartVideo {
        server: String,
        video: String,
        position: f64,
    },
    Play,
    Pause,
    Seek {
        position: f64,
    },
    Stop,
}

#[get("/cast")]
pub(super) async fn cast(
    req: HttpRequest,
    params: Query<CastParams>,
    body: Payload,
    ThinData(service_data): ThinData<ServiceData>,
) -> actix_web::Result<impl Responder> {
    let Some(device) = service_data.cast_manager.device(&params.device).await else {
        return Ok(HttpResponse::NotFound().finish());
    };

    // The cast device needs an address it can reach this server on, which is
    // not necessarily the one the browser used, e.g. when the cast device is on
    // a different network interface.
    let url_base = if let Some(url_base) = device.url_base(service_data.http_port) {
        url_base
    } else if let Some(conn_info) = req.conn_data::<ConnectionInfo>() {
        if Some(conn_info.local_addr.port()) == service_data.https_port {
            format!(
                "http://{}:{}",
                conn_info.local_addr.ip(),
                service_data.http_port
            )
        } else {
            format!("http://{}", conn_info.local_addr)
        }
    } else {
        "".to_string()
    };

    let mut cast_events = device.subscribe();

    let (response, session, ws_stream) = actix_ws::handle(&req, body)?;
    let mut ws_session = session.with_codec::<CastStateEvent, JsonCodec>(JsonCodec::new());
    let mut ws_stream = ws_stream.with_codec::<CastRequest, JsonCodec>(JsonCodec::new());
    let flick_sync = service_data.flick_sync.clone();

    actix_web::rt::spawn(async move {
        let last_event = device.last_event().await;
        if let Err(e) = ws_session.send(&last_event).await {
            warn!(error = ?e, "Failed to send cast event to websocket");
            return;
        }

        loop {
            tokio::select! {
                ws_message = ws_stream.recv() => {
                    let Some(Ok(msg)) = ws_message else {
                        break;
                    };

                    match msg {
                        CodecMessage::Ping(bytes) => {
                            if ws_session.session_mut().pong(&bytes).await.is_err() {
                                return;
                            }
                        }
                        CodecMessage::Item(CastRequest::StartVideo { server, video, position }) => {
                            if let Some(server) = flick_sync.server(&server).await
                                && let Some(video) = server.video(&video).await {
                                    device.load(&url_base, video, position).await;
                                }
                        }
                        CodecMessage::Item(CastRequest::Play) => {
                            device.play().await;
                        }
                        CodecMessage::Item(CastRequest::Pause) => {
                            device.pause().await
                        }
                        CodecMessage::Item(CastRequest::Seek { position }) => {
                            device.seek(position).await
                        }
                        CodecMessage::Item(CastRequest::Stop) => {
                            device.stop().await;
                        }
                        _ => break,
                    }
                }
                cast_event = cast_events.recv() => {
                    if let Ok(event) = cast_event {
                        if let Err(e) = ws_session.send(&event).await {
                            warn!(error = ?e, "Failed to send cast event to websocket");
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
        }

        let _ = ws_session.close(None).await;
    });

    Ok(response)
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::local_addr_for;

    #[test]
    fn local_addr_for_loopback() {
        let loopback = IpAddr::V4(Ipv4Addr::LOCALHOST);
        assert_eq!(local_addr_for(loopback), Some(loopback));
    }
}
