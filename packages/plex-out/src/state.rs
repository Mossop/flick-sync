use std::collections::{HashMap, HashSet};

use plex_api::{Collection, Episode, MetadataItem, Movie, Playlist, Season, Show};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CollectionState {
    pub id: u32,
    pub title: String,
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub items: HashSet<String>,
}

impl CollectionState {
    pub fn from_collection<T>(collection: &Collection<T>) -> Self {
        Self {
            id: collection.rating_key(),
            title: collection.title().to_owned(),
            items: Default::default(),
        }
    }

    pub fn update_from_collection<T>(&mut self, collection: &Collection<T>) {
        self.title = collection.title().to_owned();
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistState {
    pub id: u32,
    pub title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub videos: Vec<String>,
}

impl PlaylistState {
    pub fn from_playlist<T>(playlist: &Playlist<T>) -> Self {
        Self {
            id: playlist.rating_key(),
            title: playlist.title().to_owned(),
            videos: Default::default(),
        }
    }

    pub fn update_from_playlist<T>(&mut self, playlist: &Playlist<T>) {
        self.title = playlist.title().to_owned();
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(tag = "type", content = "content", rename_all = "lowercase")]
pub enum LibraryContent {
    Movies(HashSet<String>),
    Shows(HashMap<String, ShowState>),
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LibraryState {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub collections: HashMap<String, CollectionState>,
    #[serde(flatten)]
    pub content: LibraryContent,
    pub path: String,
}

impl LibraryState {
    pub fn add_movie(&mut self, movie: &Movie) {
        match self.content {
            LibraryContent::Movies(ref mut movies) => {
                movies.insert(movie.rating_key().to_string());
            }
            _ => panic!("Unexpected library type."),
        }
    }

    pub fn add_episode(&mut self, show: &Show, season: &Season, episode: &Episode) {
        match self.content {
            LibraryContent::Shows(ref mut shows) => {
                let show_state = shows
                    .entry(show.rating_key().to_string())
                    .and_modify(|ss| ss.update_from_show(show))
                    .or_insert_with(|| ShowState::from_show(show));

                let season_state = show_state
                    .seasons
                    .entry(season.rating_key().to_string())
                    .and_modify(|ss| ss.update_from_season(season))
                    .or_insert_with(|| SeasonState::from_season(season));

                season_state
                    .episodes
                    .insert(episode.rating_key().to_string());
            }
            _ => panic!("Unexpected library type."),
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SeasonState {
    pub id: u32,
    pub index: u32,
    pub title: String,
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub episodes: HashSet<String>,
}

impl SeasonState {
    pub fn from_season(season: &Season) -> Self {
        Self {
            id: season.rating_key(),
            index: season.metadata().index.unwrap(),
            title: season.title().to_owned(),
            episodes: Default::default(),
        }
    }

    pub fn update_from_season(&mut self, season: &Season) {
        self.index = season.metadata().index.unwrap();
        self.title = season.title().to_owned();
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ShowState {
    pub id: u32,
    pub title: String,
    pub year: u32,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub seasons: HashMap<String, SeasonState>,
    pub path: String,
}

impl ShowState {
    pub fn from_show(show: &Show) -> Self {
        let metadata = show.metadata();

        let year = metadata.year.unwrap();
        let title = show.title().to_owned();
        let path = format!("{title} ({year})");

        Self {
            id: show.rating_key(),
            title,
            year,
            seasons: Default::default(),
            path,
        }
    }

    pub fn update_from_show(&mut self, show: &Show) {
        let metadata = show.metadata();

        self.year = metadata.year.unwrap();
        self.title = show.title().to_owned();
    }
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
pub enum VideoType {
    Movie,
    Episode,
}

fn is_zero(val: &u32) -> bool {
    *val == 0
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VideoState {
    pub id: u32,
    pub title: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub year: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub index: u32,
    #[serde(rename = "type")]
    pub video_type: VideoType,
    pub file_prefix: String,
}

impl VideoState {
    pub fn from_movie(movie: &Movie) -> Self {
        let metadata = movie.metadata();

        let year = metadata.year.unwrap();
        let title = movie.title().to_owned();
        let file_prefix = format!("{title} ({year})");

        Self {
            id: movie.rating_key(),
            title,
            year,
            index: 0,
            video_type: VideoType::Movie,
            file_prefix,
        }
    }

    pub fn update_from_movie(&mut self, movie: &Movie) {
        let metadata = movie.metadata();

        self.year = metadata.year.unwrap();
        self.title = movie.title().to_owned();
        self.video_type = VideoType::Movie;
    }

    pub fn from_episode(episode: &Episode) -> Self {
        let metadata = episode.metadata();

        let season = metadata.parent.parent_index.unwrap();
        let index = metadata.index.unwrap();

        let title = episode.title().to_owned();
        let file_prefix = format!("S{season}E{index} - {title}");

        Self {
            id: episode.rating_key(),
            title,
            year: 0,
            index: metadata.index.unwrap(),
            video_type: VideoType::Episode,
            file_prefix,
        }
    }

    pub fn update_from_episode(&mut self, episode: &Episode) {
        let metadata = episode.metadata();

        self.title = episode.title().to_owned();
        self.video_type = VideoType::Episode;
        self.index = metadata.index.unwrap();
    }
}

#[derive(Deserialize, Default, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ServerState {
    pub token: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub playlists: HashMap<String, PlaylistState>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub libraries: HashMap<String, LibraryState>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub videos: HashMap<String, VideoState>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct State {
    pub client_id: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub servers: HashMap<String, ServerState>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            client_id: Uuid::new_v4().braced().to_string(),
            servers: Default::default(),
        }
    }
}
