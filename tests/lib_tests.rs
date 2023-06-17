use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDateTime, Utc};
// use bigquery_googleapi::BigqueryClient;
use downloader::prelude::*;
use google_bigquery_v2::prelude::*;

use downloader;
use downloader::data::{Streamers, VideoData, VideoMetadata, Videos};
use downloader::{
    get_playlist_title_from_twitch_video, get_video_prefix_from_twitch_video,
    get_video_title_from_twitch_video, MAX_VIDEO_TITLE_LENGTH, PART_PREFIX_LENGTH,
};

fn init_console_logging(log_level: LevelFilter) {
    let _ = env_logger::builder()
        .filter_level(log_level)
        .is_test(true)
        .try_init();
}

async fn get_sample_client() -> BigqueryClient {
    BigqueryClient::new(
        "twitchbackup-v1",
        "backup_data",
        Some("tests/test_data/auth/bigquery_service_account.json"),
    )
    .await
    .unwrap()
}

fn get_sample_video(client: &BigqueryClient) -> VideoData {
    VideoData {
        video: Videos {
            created_at: Some(get_utc_from_string("2021-01-01T00:00:00")),
            video_id: 1,
            client: client.clone(),
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
            client: client.clone(),
            backed_up: Some(false),
            ..Default::default()
        },
        streamer: Streamers {
            display_name: Some("NoPixel VODs".to_string()),
            login: "nopixelvods".to_string(),
            client: client.clone(),
            youtube_user: Some("NoPixel VODs".to_string()),
            watched: Some(true),
            public_videos_default: Some(false),
            youtube_google_ident: None,
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

const LONG_TITLE_ONLY_EMOJI: &'static str = "🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠\
                                             🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠\
                                             🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠\
                                             🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠\
                                             🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠\
                                             🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠\
                                             🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠\
                                             🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠\
                                             🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠";

#[tokio::test]
async fn get_video_title() {
    init_console_logging(LevelFilter::Debug);
    let client = get_sample_client().await;
    let video = get_sample_video(&client);

    let title = get_video_title_from_twitch_video(&video, 5, 20).unwrap();
    assert_eq!(title, "[2021-01-01][Part 05/20] Test Video");
}
#[tokio::test]
async fn get_video_long_title() {
    init_console_logging(LevelFilter::Debug);
    let client = get_sample_client().await;
    let mut video = get_sample_video(&client);
    video.video.title = Some(LONG_TITLE.to_string());
    let title = get_video_title_from_twitch_video(&video, 5, 20).unwrap();
    info!("part title: {}", title);
    assert_eq!(title, "[2021-01-01][Part 05/20] long title with over a hundred characters that is definitely going to be...");
}
#[tokio::test]
async fn get_video_long_title_only_emoji() {
    init_console_logging(LevelFilter::Debug);
    let client = get_sample_client().await;
    let mut video = get_sample_video(&client);
    video.video.title = Some(LONG_TITLE_ONLY_EMOJI.to_string());
    let title = get_video_title_from_twitch_video(&video, 1, 1).unwrap();
    info!("part title: {}", title);
    assert_eq!(
        MAX_VIDEO_TITLE_LENGTH - PART_PREFIX_LENGTH,
        title.chars().count()
    ); //this is 88 chars long to leave space for the part prefix part, with that it should be exactly 100 chars
    assert_eq!(
        "[2021-01-01] 🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠...",
        title,
    );
    video.video.title = Some(LONG_TITLE_ONLY_EMOJI.to_string());
    let title = get_video_title_from_twitch_video(&video, 5, 20).unwrap();
    info!("part title: {}", title);
    assert_eq!(MAX_VIDEO_TITLE_LENGTH, title.chars().count());
    assert_eq!(
        "[2021-01-01][Part 05/20] 🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠🔴🟠...",
        title,
    );
}

#[tokio::test]
async fn get_video_title_single_part() {
    init_console_logging(LevelFilter::Debug);
    let client = get_sample_client().await;
    let video = get_sample_video(&client);

    let title = get_video_title_from_twitch_video(&video, 1, 1).unwrap();
    assert_eq!(title, "[2021-01-01] Test Video");
}
#[tokio::test]
async fn get_video_long_title_single_part() {
    init_console_logging(LevelFilter::Debug);
    let client = get_sample_client().await;
    let mut video = get_sample_video(&client);

    video.video.title = Some(LONG_TITLE.to_string());
    let title = get_video_title_from_twitch_video(&video, 1, 1).unwrap();
    info!("single part title: {}", title);
    assert_eq!(
        title,
        "[2021-01-01] long title with over a hundred characters that is definitely going to be..."
    );
}

#[tokio::test]
async fn get_playlist_title() {
    init_console_logging(LevelFilter::Debug);
    let client = get_sample_client().await;
    let video = get_sample_video(&client);

    let title = get_playlist_title_from_twitch_video(&video).unwrap();
    assert_eq!("[2021-01-01] Test Video", title);
}

#[tokio::test]
async fn get_playlist_long_title() {
    init_console_logging(LevelFilter::Debug);
    let client = get_sample_client().await;
    let mut video = get_sample_video(&client);

    video.video.title = Some(LONG_TITLE.to_string());
    let title = get_playlist_title_from_twitch_video(&video).unwrap();
    info!("playlist title: {}", title);
    assert_eq!(
        "[2021-01-01] long title with over a hundred characters that is definitel...",
        title
    );
}

#[tokio::test]
async fn get_video_prefix() {
    init_console_logging(LevelFilter::Debug);
    let client = get_sample_client().await;
    let video = get_sample_video(&client);

    let prefix = get_video_prefix_from_twitch_video(&video, 5, 20).unwrap();
    info!("prefix: {}", prefix);
    assert_eq!(prefix, "[2021-01-01][Part 05/20]");
}

#[tokio::test]
async fn split_video_into_parts_with_join() {
    init_console_logging(LevelFilter::Debug);
    let (tmp_folder_path, video_path) = prepare_existing_video_test_data(1);

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
    debug!("parts: {:?}", parts);
    assert_eq!(5, parts.len(),);
    for (i, part) in parts.iter().enumerate() {
        assert_eq!(
            format!("short_video_00{}.mp4", i),
            part.file_name().unwrap().to_str().unwrap()
        );
    }
}

#[tokio::test]
async fn split_video_into_parts_without_join() {
    init_console_logging(LevelFilter::Debug);
    let (tmp_folder_path, video_path) = prepare_existing_video_test_data(2);

    let parts = downloader::split_video_into_parts(
        PathBuf::from(&video_path),
        chrono::Duration::seconds(5),
        chrono::Duration::seconds(6),
    )
    .await;

    //region clean up
    std::fs::remove_dir_all(tmp_folder_path).unwrap();
    //endregion

    let parts = parts.expect("failed to split video into parts");
    debug!("parts: {:?}", parts);
    assert_eq!(6, parts.len(),);
    for (i, part) in parts.iter().enumerate() {
        assert_eq!(
            format!("short_video_00{}.mp4", i),
            part.file_name().unwrap().to_str().unwrap()
        );
    }
}

fn prepare_existing_video_test_data(temp_subname: i32) -> (PathBuf, PathBuf) {
    let video_source = Path::new("tests/test_data/short_video/short_video.mp4");
    let tmp_folder_path = format!("tests/test_data/tmp_{}", temp_subname);
    let tmp_folder_path: PathBuf = tmp_folder_path.as_str().into();
    let video_path = Path::join(&tmp_folder_path, "short_video/short_video.mp4");
    if tmp_folder_path.exists() {
        std::fs::remove_dir_all(&tmp_folder_path).unwrap();
    }
    std::fs::create_dir_all(video_path.parent().unwrap()).unwrap();
    std::fs::copy(video_source, &video_path).unwrap();
    (tmp_folder_path.to_path_buf(), video_path)
}
