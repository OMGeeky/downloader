use std::any::Any;
use std::error::Error;
use std::fmt::Debug;

use chrono::DateTime;
use chrono::Utc;
use google_bigquery_v2::data::query_builder::QueryResultType;
use google_bigquery_v2::prelude::*;

#[derive(BigDataTableDerive, Debug, Default, Clone)]
#[db_name("streamers")]
pub struct Streamers {
    #[primary_key]
    #[required]
    pub login: String,
    #[client]
    pub client: BigqueryClient,
    pub display_name: Option<String>,
    pub watched: Option<bool>,
    pub youtube_user: Option<String>,
    pub public_videos_default: Option<bool>,
    pub youtube_google_ident: Option<String>,
}

#[derive(BigDataTableDerive, Debug, Default, Clone)]
#[db_name("videos")]
pub struct Videos {
    #[primary_key]
    #[required]
    pub video_id: i64,
    #[client]
    pub client: BigqueryClient,

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

#[derive(BigDataTableDerive, Debug, Default, Clone)]
#[db_name("video_metadata")]
pub struct VideoMetadata {
    #[primary_key]
    #[required]
    pub video_id: i64,
    #[client]
    pub client: BigqueryClient,

    pub backed_up: Option<bool>,
    pub total_clips_amount: Option<i64>,
    pub parts_backed_up_id: Option<i64>,
    pub parts_size: Option<i64>,
    pub error: Option<String>,
    pub download_playlist_url: Option<String>,
    pub youtube_playlist_url: Option<String>,
}

#[derive(Debug, Default)]
pub struct VideoData {
    pub video: Videos,
    pub metadata: VideoMetadata,
    pub streamer: Streamers,
}

impl VideoData {
    pub fn from_twitch_video(video: &twitch_data::Video, client: BigqueryClient) -> Result<Self> {
        Ok(Self {
            video: Videos {
                video_id: video.id.parse::<i64>()?,
                client: client.clone(),
                title: Some(video.title.clone()),
                description: Some(video.description.clone()),
                bool_test: Some(true),
                user_login: Some(video.user_login.to_lowercase()),
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
                client: client.clone(),
                backed_up: Some(false),
                ..Default::default()
            },
            streamer: Streamers {
                ..Default::default()
            },
        })
    }
}
