#![allow(unused, incomplete_features)]

use std::error::Error;
use std::fmt::Debug;
use std::path::Path;

use google_bigquery;
use google_bigquery::{BigDataTable, BigqueryClient};
use google_youtube::scopes;
use google_youtube::YoutubeClient;
use log::{debug, error, info, trace, warn};
use nameof::name_of;
use simplelog::*;
use tokio::fs::File;
use twitch_data::{
    convert_twitch_video_to_twitch_data_video, get_client, TwitchClient, Video, VideoQuality,
};

use downloader::data::{Streamers, VideoMetadata};
use downloader::start_backup;

//region constants

const SERVICE_ACCOUNT_PATH: &str = "auth/bigquery_service_account.json";
const YOUTUBE_CLIENT_SECRET: &str = "auth/youtube_client_secret.json";
const PROJECT_ID: &str = "twitchbackup-v1";
const DATASET_ID: &str = "backup_data";
//endregion

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let log_folder = "downloader/logs/";
    tokio::fs::create_dir_all(log_folder).await?;
    let timestamp = chrono::Utc::now().format("%Y-%m-%d_%H-%M-%S").to_string();

    CombinedLogger::init(vec![
        // SimpleLogger::new(LevelFilter::Info, Config::default()),
        TermLogger::new(
            LevelFilter::Info,
            Config::default(),
            TerminalMode::Mixed,
            ColorChoice::Auto,
        ),
        WriteLogger::new(
            LevelFilter::Info,
            Config::default(),
            File::create(format!("{}downloader_{}.log", log_folder, timestamp))
                .await?
                .into_std()
                .await,
        ),
        WriteLogger::new(
            LevelFilter::Trace,
            Config::default(),
            File::create(format!("{}trace_{}.log", log_folder, timestamp))
                .await?
                .into_std()
                .await,
        ),
    ])
    .unwrap();
    log_panics::init();
    println!("Hello, world!");
    start_backup().await?;
    // sample().await?;
    Ok(())
}

pub async fn sample() -> Result<(), Box<dyn Error>> {
    info!("Hello from the downloader lib!");

    let client = BigqueryClient::new(PROJECT_ID, DATASET_ID, Some(SERVICE_ACCOUNT_PATH)).await?;
    sample_bigquery(&client).await?;

    /*
        let youtube_client = YoutubeClient::new(Some(YOUTUBE_CLIENT_SECRET),
                                                vec![scopes::YOUTUBE_UPLOAD, scopes::YOUTUBE_READONLY, scopes::YOUTUBE],
                                                Some("NopixelVODs")).await?;

        sample_youtube(&youtube_client).await?;
    */
    /*
    let twitch_client = get_client().await?;
    sample_twitch(&twitch_client).await?;
    */
    Ok(())
}

async fn sample_twitch<'a>(client: &TwitchClient<'a>) -> Result<(), Box<dyn Error>> {
    info!("\n\nGetting videos...");

    let res = client.get_channel_info_from_login("burn").await?;
    info!("got channel info: {:?}", res);
    let channel_id = res.unwrap().broadcaster_id;

    let videos: Vec<Video> = client
        .get_videos_from_channel(&channel_id, 50)
        .await?
        .into_iter()
        .map(convert_twitch_video_to_twitch_data_video)
        .collect();

    info!("got video ids: {:?}", videos.len());
    for (i, video) in videos.iter().enumerate() {
        info!("+======={:2}: {:?}", i, video);
    }

    info!("\n\nGetting video for short download...");
    let short_video_id = twitch_data::VideoId::new("1710229470".to_string());
    let video_info = client.get_video_info(&short_video_id).await?;
    info!("got video info: {:?}", video_info);

    let output_folder = Path::new("C:\\tmp\\videos\\");
    let res = client
        .download_video_by_id(&video_info.id, &VideoQuality::Source, output_folder)
        .await?;
    info!("downloaded video: {:?}", res);

    info!("\n\nDone!");
    Ok(())
}

async fn sample_bigquery<'a>(client: &'a BigqueryClient) -> Result<(), Box<dyn Error>> {
    // let x = VideoMetadata::from_pk(&client, 1638184921).await?;
    let video_metadata = VideoMetadata::create_and_load_from_pk(&client, 1638184921).await?;
    info!("got video_metadata by id: {:?}", video_metadata);

    let video_metadata = VideoMetadata::load_by_field(
        &client,
        name_of!(backed_up in VideoMetadata),
        Some(true),
        10,
    )
    .await?;
    print_vec_sample("got video_metadata by backed_up:", video_metadata);

    let watched_streamers =
        Streamers::load_by_field(&client, name_of!(watched in Streamers), Some(true), 100).await?;
    print_vec_sample("got watched_streamers:", watched_streamers);

    fn print_vec_sample<T: Debug>(message: &str, watched_streamers: Vec<T>) {
        info!("{} {:?}", message, watched_streamers.len());
        for (i, streamer) in watched_streamers.iter().enumerate() {
            info!("+======={}: {:?}", i, streamer);
        }
    }
    Ok(())
}

async fn sample_youtube(client: &YoutubeClient) -> Result<(), Box<dyn Error>> {
    info!("Opening video file...");
    let file = Path::new("C:\\Users\\frede\\Videos\\test.mp4");
    // let file = File::open(file).await?;

    let description = "test video description";
    let title = "test video2";
    let tags = vec!["test".to_string(), "test2".to_string()];
    let privacy_status = google_youtube::PrivacyStatus::Private;

    info!("Uploading video...");
    let video = &client
        .upload_video(file, title, description, tags, privacy_status)
        .await?;

    info!("video: \n\n{:?}\n\n", video);

    Ok(())
}
