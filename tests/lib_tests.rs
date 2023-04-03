use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDateTime, Utc};
// use bigquery_googleapi::BigqueryClient;
use google_bigquery::BigqueryClient;
use log::LevelFilter;
use simplelog::{ColorChoice, TermLogger, TerminalMode};

use downloader;
use downloader::data::{Streamers, VideoData, VideoMetadata, Videos};
use downloader::{
    get_playlist_title_from_twitch_video, get_video_prefix_from_twitch_video,
    get_video_title_from_twitch_video,
};

fn init_console_logging(log_level: LevelFilter) {
    TermLogger::init(
        log_level,
        simplelog::Config::default(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )
    .unwrap();
}

async fn get_sample_client() -> BigqueryClient {
    BigqueryClient::new(
        "twitchbackup-v1",
        "backup_data",
        Some("auth/bigquery_service_account.json"),
    )
    .await
    .unwrap()
}

fn get_sample_video(client: &BigqueryClient) -> VideoData {
    VideoData {
        video: Videos {
            created_at: Some(get_utc_from_string("2021-01-01T00:00:00")),
            video_id: 1,
            client: Some(client),
            title: Some("Test Video".to_string()),
            description: Some("Test Description".to_string()),
            bool_test: Some(true),
            user_login: Some("nopixelvods".to_string()),
            url: Some("https://www.twitch.tv/videos/1".to_string()),
            viewable: Some("public".to_string()),
            language: Some("en".to_string()),
            view_count: Some(1),
            video_type: Some("archive".to_string()),
            duration: Some(1),
            thumbnail_url: Some("i dont know".to_string()),
        },
        metadata: VideoMetadata {
            video_id: 1,
            client: Some(client),
            backed_up: Some(false),
            ..Default::default()
        },
        streamer: Streamers {
            display_name: Some("NoPixel VODs".to_string()),
            login: "nopixelvods".to_string(),
            client: Some(client),
            youtube_user: Some("NoPixel VODs".to_string()),
            watched: Some(true),
            public_videos_default: Some(false),
        },
    }
}

fn get_utc_from_string(s: &str) -> DateTime<Utc> {
    let n = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").unwrap();
    let utc = DateTime::<Utc>::from_utc(n, Utc);
    utc
}

const LONG_TITLE: &'static str =
    "long title with over a hundred characters that is definitely going to \
    be cut of because it does not fit into the maximum length that youtube requires";

#[tokio::test]
async fn get_video_title() {
    let client = get_sample_client().await;
    let mut video = get_sample_video(&client);

    let title = get_video_title_from_twitch_video(&video, 5, 20).unwrap();
    assert_eq!(title, "[2021-01-01][Part 05/20] Test Video");

    video.video.title = Some(LONG_TITLE.to_string());
    let title = get_video_title_from_twitch_video(&video, 5, 20).unwrap();
    println!("part title:\n{}", title);
    assert_eq!(title, "[2021-01-01][Part 05/20] long title with over a hundred characters that is definitely going to be...");
}

#[tokio::test]
async fn get_video_title_single_part() {
    let client = get_sample_client().await;
    let mut video = get_sample_video(&client);

    let title = get_video_title_from_twitch_video(&video, 1, 1).unwrap();
    assert_eq!(title, "Test Video");

    video.video.title = Some(LONG_TITLE.to_string());
    let title = get_video_title_from_twitch_video(&video, 1, 1).unwrap();
    println!("single part title:\n{}", title);
    assert_eq!(
        title,
        "long title with over a hundred characters that is definitely going to be..."
    );
}

#[tokio::test]
async fn get_playlist_title() {
    let client = get_sample_client().await;
    let mut video = get_sample_video(&client);

    let title = get_playlist_title_from_twitch_video(&video).unwrap();
    assert_eq!(title, "Test Video");

    video.video.title = Some(LONG_TITLE.to_string());
    let title = get_playlist_title_from_twitch_video(&video).unwrap();
    println!("playlist title:\n{}", title);
    assert_eq!(
        title,
        "long title with over a hundred characters that is definitely going to be..."
    );
}

#[tokio::test]
async fn get_video_prefix() {
    let client = get_sample_client().await;
    let video = get_sample_video(&client);

    let prefix = get_video_prefix_from_twitch_video(&video, 5, 20).unwrap();
    println!("prefix:\n{}", prefix);
    assert_eq!(prefix, "[2021-01-01][Part 05/20]");
}

#[tokio::test]
async fn split_video_into_parts() {
    init_console_logging(LevelFilter::Debug);

    //region prepare test data
    let video_source = Path::new("tests/test_data/short_video/short_video.mp4");
    let tmp_folder_path = Path::new("tests/test_data/tmp/");
    let video_path = Path::join(tmp_folder_path, "short_video/short_video.mp4");
    if tmp_folder_path.exists() {
        std::fs::remove_dir_all(tmp_folder_path).unwrap();
    }
    std::fs::create_dir_all(video_path.parent().unwrap()).unwrap();
    std::fs::copy(video_source, &video_path).unwrap();
    //endregion

    let parts = downloader::split_video_into_parts(
        PathBuf::from(&video_path),
        chrono::Duration::seconds(5),
        chrono::Duration::seconds(9),
    )
    .await;

    //region clean up
    std::fs::remove_dir_all(tmp_folder_path).unwrap();
    //endregion

    let parts = parts.expect("failed to split video into parts");
    println!("parts: {:?}", parts);
    assert_eq!(parts.len(), 5);
}
