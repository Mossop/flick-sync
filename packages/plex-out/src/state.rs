use std::collections::{HashMap, HashSet};

use plex_api::{Collection, Episode, MetadataItem, Movie, Playlist, Season, Show};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CollectionState {
    pub title: String,
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub items: HashSet<String>,
}

impl CollectionState {
    pub fn from_collection<T>(collection: &Collection<T>) -> Self {
        Self {
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
    pub title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub videos: Vec<String>,
}

impl PlaylistState {
    pub fn from_playlist<T>(playlist: &Playlist<T>) -> Self {
        Self {
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
    pub title: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub collections: HashMap<String, CollectionState>,
    #[serde(flatten)]
    pub content: LibraryContent,
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
    pub index: u32,
    pub title: String,
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub episodes: HashSet<String>,
}

impl SeasonState {
    pub fn from_season(season: &Season) -> Self {
        Self {
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
    pub title: String,
    pub year: u32,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub seasons: HashMap<String, SeasonState>,
}

impl ShowState {
    pub fn from_show(show: &Show) -> Self {
        let metadata = show.metadata();

        let year = metadata.year.unwrap();
        let title = show.title().to_owned();

        Self {
            title,
            year,
            seasons: Default::default(),
        }
    }

    pub fn update_from_show(&mut self, show: &Show) {
        let metadata = show.metadata();

        self.year = metadata.year.unwrap();
        self.title = show.title().to_owned();
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MovieState {
    pub title: String,
    pub library: String,
    pub year: u32,
    pub file_prefix: String,
}

impl MovieState {
    fn generate_prefix(movie: &Movie) -> String {
        let metadata = movie.metadata();

        let library_id = metadata.library_section_id.unwrap().to_string();
        let year = metadata.year.unwrap();
        let title = movie.title().to_owned();
        format!("{library_id}/{title} ({year})/{title} ({year})")
    }

    pub fn from_movie(movie: &Movie) -> Self {
        let metadata = movie.metadata();

        MovieState {
            title: movie.title().to_owned(),
            library: metadata.library_section_id.unwrap().to_string(),
            year: metadata.year.unwrap(),
            file_prefix: Self::generate_prefix(movie),
        }
    }

    pub fn update_from_movie(&mut self, movie: &Movie) {
        let metadata = movie.metadata();

        self.year = metadata.year.unwrap();
        self.title = movie.title().to_owned();

        let prefix = Self::generate_prefix(movie);
        if prefix != self.file_prefix {
            log::warn!("File path for {} changed.", self.title);
        }
        self.file_prefix = prefix;
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeState {
    pub title: String,
    pub library: String,
    pub show: String,
    pub season: String,
    pub index: u32,
    pub file_prefix: String,
}

impl EpisodeState {
    fn generate_prefix(show: &Show, episode: &Episode) -> String {
        let metadata = episode.metadata();

        let library_id = show.metadata().library_section_id.unwrap().to_string();
        let show_title = show.title();
        let show_year = show.metadata().year.unwrap();
        let season_index = metadata.parent.parent_index.unwrap();
        let index = metadata.index.unwrap();
        let title = episode.title().to_owned();
        format!("{library_id}/{show_title} ({show_year})/S{season_index:02}E{index:02} - {title}")
    }

    pub fn from_episode(show: &Show, season: &Season, episode: &Episode) -> Self {
        let metadata = episode.metadata();

        EpisodeState {
            title: episode.title().to_owned(),
            library: show.metadata().library_section_id.unwrap().to_string(),
            show: show.rating_key().to_string(),
            season: season.rating_key().to_string(),
            index: metadata.index.unwrap(),
            file_prefix: Self::generate_prefix(show, episode),
        }
    }

    pub fn update_from_episode(&mut self, show: &Show, season: &Season, episode: &Episode) {
        let metadata = episode.metadata();

        self.title = episode.title().to_owned();
        self.show = show.rating_key().to_string();
        self.season = season.rating_key().to_string();
        self.index = metadata.index.unwrap();

        let prefix = Self::generate_prefix(show, episode);
        if prefix != self.file_prefix {
            log::warn!("File path for {} changed.", self.title);
        }
        self.file_prefix = prefix;
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum VideoState {
    Movie(MovieState),
    Episode(EpisodeState),
}

impl VideoState {
    pub fn from_movie(movie: &Movie) -> Self {
        Self::Movie(MovieState::from_movie(movie))
    }

    pub fn update_from_movie(&mut self, movie: &Movie) {
        match self {
            Self::Movie(m) => m.update_from_movie(movie),
            _ => panic!("Unexpected video type"),
        }
    }

    pub fn from_episode(show: &Show, season: &Season, episode: &Episode) -> Self {
        VideoState::Episode(EpisodeState::from_episode(show, season, episode))
    }

    pub fn update_from_episode(&mut self, show: &Show, season: &Season, episode: &Episode) {
        match self {
            Self::Episode(e) => {
                e.update_from_episode(show, season, episode);
            }
            _ => panic!("Unexpected video type"),
        }
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
