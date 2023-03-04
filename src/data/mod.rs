use std::any::Any;
use std::error::Error;
use std::fmt::Debug;

use chrono::DateTime;
use chrono::Utc;
use google_bigquery::utils::ConvertValueToBigqueryParamValue;
use google_bigquery::{
    BigDataTable, BigDataTableBase, BigDataTableBaseConvenience, BigDataTableDerive,
    BigDataTableHasPk, BigqueryClient, HasBigQueryClient, HasBigQueryClientDerive,
};

#[derive(BigDataTableDerive, HasBigQueryClientDerive)]
#[db_name("streamers")]
pub struct Streamers<'a> {
    #[primary_key]
    #[required]
    pub login: String,
    #[client]
    pub client: Option<&'a BigqueryClient>,
    pub display_name: Option<String>,
    pub watched: Option<bool>,
    pub youtube_user: Option<String>,
    pub public_videos_default: Option<bool>,
}

impl Debug for Streamers<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Streamers")
            .field("login", &self.login)
            .field("display_name", &self.display_name)
            .field("watched", &self.watched)
            .field("youtube_user", &self.youtube_user)
            .finish()
    }
}

/*impl<'a> Streamers<'a> {
    pub fn get_watched(&self) -> String {
        self.login.clone()
    }

    fn get_pk_value(&self) -> String {
        self.login.clone()
    }
}*/

impl Default for Streamers<'_> {
    fn default() -> Self {
        Self {
            login: "".to_string(),
            client: None,
            display_name: None,
            watched: None,
            youtube_user: None,
            public_videos_default: None,
        }
    }
}

#[derive(BigDataTableDerive, HasBigQueryClientDerive)]
#[db_name("videos")]
pub struct Videos<'a> {
    #[primary_key]
    #[required]
    pub video_id: i64,
    #[client]
    pub client: Option<&'a BigqueryClient>,

    pub title: Option<String>,
    pub description: Option<String>,
    pub bool_test: Option<bool>,
    pub user_login: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub url: Option<String>,
    pub viewable: Option<String>,
    pub language: Option<String>,
    pub view_count: Option<i64>,
    pub video_type: Option<String>,
    pub duration: Option<i64>,
    pub thumbnail_url: Option<String>,
}

impl Debug for Videos<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Videos")
            .field("video_id", &self.video_id)
            .field("title", &self.title)
            .field("description", &self.description)
            .field("bool_test", &self.bool_test)
            .field("user_login", &self.user_login)
            .field("created_at", &self.created_at)
            .field("url", &self.url)
            .field("viewable", &self.viewable)
            .field("language", &self.language)
            .field("view_count", &self.view_count)
            .field("video_type", &self.video_type)
            .field("duration", &self.duration)
            .field("thumbnail_url", &self.thumbnail_url)
            .finish()
    }
}

impl Default for Videos<'_> {
    fn default() -> Self {
        Self {
            video_id: -9999,
            client: None,
            title: None,
            description: None,
            bool_test: None,
            user_login: None,
            created_at: None,
            url: None,
            viewable: None,
            language: None,
            view_count: None,
            video_type: None,
            duration: None,
            thumbnail_url: None,
        }
    }
}

#[derive(BigDataTableDerive, HasBigQueryClientDerive)]
#[db_name("video_metadata")]
pub struct VideoMetadata<'a> {
    #[primary_key]
    #[required]
    pub video_id: i64,
    #[client]
    pub client: Option<&'a BigqueryClient>,

    pub backed_up: Option<bool>,
    pub total_clips_amount: Option<i64>,
    pub parts_backed_up_id: Option<i64>,
    pub parts_size: Option<i64>,
    pub error: Option<String>,
    pub download_playlist_url: Option<String>,
    pub youtube_playlist_url: Option<String>,
}

impl Debug for VideoMetadata<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoMetadata")
            .field("video_id", &self.video_id)
            .field("backed_up", &self.backed_up)
            .field("total_clips_amount", &self.total_clips_amount)
            .field("parts_backed_up_id", &self.parts_backed_up_id)
            .field("parts_size", &self.parts_size)
            .field("error", &self.error)
            .field("download_playlist_url", &self.download_playlist_url)
            .field("youtube_playlist_url", &self.youtube_playlist_url)
            .finish()
    }
}

impl Default for VideoMetadata<'_> {
    fn default() -> Self {
        Self {
            video_id: -9999,
            client: None,
            error: None,
            backed_up: None,
            total_clips_amount: None,
            parts_backed_up_id: None,
            parts_size: None,
            download_playlist_url: None,
            youtube_playlist_url: None,
        }
    }
}

#[derive(Debug, Default)]
pub struct VideoData<'a> {
    pub video: Videos<'a>,
    pub metadata: VideoMetadata<'a>,
    pub streamer: Streamers<'a>,
}
impl<'a> VideoData<'a> {
    pub fn from_twitch_video(
        video: &twitch_data::Video,
        client: &'a BigqueryClient,
    ) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            video: Videos {
                video_id: video.id.parse::<i64>()?,
                client: Some(client),
                title: Some(video.title.clone()),
                description: Some(video.description.clone()),
                bool_test: Some(true),
                user_login: Some(video.user_login.to_string()),
                created_at: Some(video.created_at.clone()),
                url: Some(video.url.clone()),
                viewable: Some(video.viewable.clone()),
                language: Some(video.language.clone()),
                view_count: Some(video.view_count),
                video_type: Some("archive".to_string()),
                duration: Some(video.duration),
                thumbnail_url: Some(video.thumbnail_url.clone()),
            },
            metadata: VideoMetadata {
                video_id: video.id.parse::<i64>()?,
                client: Some(client),
                backed_up: Some(false),
                ..Default::default()
            },
            streamer: Streamers {
                ..Default::default()
            },
        })
    }
}
