#![allow(unused, incomplete_features)]

use std::error::Error;
use std::fmt::Debug;
use std::path::Path;

use anyhow::{anyhow, Result};
use google_bigquery_v2::prelude::*;
use google_youtube::scopes;
use google_youtube::YoutubeClient;
use log::{debug, error, info, trace, warn};
use log4rs::append::console::ConsoleAppender;
use log4rs::append::rolling_file::policy::compound::roll;
use log4rs::append::rolling_file::policy::compound::roll::fixed_window::FixedWindowRoller;
use log4rs::append::rolling_file::policy::compound::trigger::size::SizeTrigger;
use log4rs::append::rolling_file::policy::{compound::CompoundPolicy, Policy};
use log4rs::append::rolling_file::{RollingFileAppender, RollingFileAppenderBuilder};
use log4rs::config::{Appender, Root};
use log4rs::encode::pattern::PatternEncoder;
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
    initialize_logger2().await;
    info!("Hello, world!");
    start_backup().await?;
    // sample().await?;
    Ok(())
}

async fn initialize_logger2() -> Result<(), Box<dyn Error>> {
    // // example:
    // // [2023-04-07T13:00:03.689100600+02:00] [INFO ] downloader::src\main.rs:42 - Hello, world!
    // let encoder = PatternEncoder::new("[{d:35}] [{h({l:5})}] {M}::{file}:{L} - {m}{n}");
    // use log4rs::append::rolling_file::policy::compound::trigger::size::SizeTriggerDeserializer;
    // let stdout = ConsoleAppender::builder()
    //     .encoder(Box::new(encoder.clone()))
    //     .build();
    // let size_trigger = SizeTrigger::new(gb_to_bytes(1.0));
    // // let size_trigger = SizeTrigger::new(1000);
    // let roller = FixedWindowRoller::builder()
    //     .build("downloader/logs/archive/downloader.{}.log", 3)
    //     .unwrap();
    // let policy = CompoundPolicy::new(Box::new(size_trigger), Box::new(roller));
    // let file = RollingFileAppender::builder()
    //     .encoder(Box::new(encoder.clone()))
    //     .build("downloader/logs/downloader.log", Box::new(policy))
    //     .unwrap();
    // let config = log4rs::Config::builder()
    //     .appender(Appender::builder().build("stdout", Box::new(stdout)))
    //     .appender(Appender::builder().build("file", Box::new(file)))
    //     .build(
    //         Root::builder()
    //             .appender("stdout")
    //             .appender("file")
    //             .build(LevelFilter::Debug),
    //     )
    //     .unwrap();
    //
    // let _handle = log4rs::init_config(config).unwrap();
    let logger_config_path = Path::new("logger.yaml");
    let path = &logger_config_path
        .canonicalize()
        .expect(format!("could not find the file: {:?}", logger_config_path).as_str());
    println!(
        "Log config file path: {:?} => {:?}",
        logger_config_path, path
    );
    log4rs::init_file(path, Default::default())
        .expect("Failed to initialize the logger from the file");
    info!("==================================================================================");
    info!(
        "Start of new log on {}",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S")
    );
    info!("==================================================================================");

    Ok(())
}

fn gb_to_bytes(gb: f32) -> u64 {
    (gb * 1000000000.0) as u64
}

async fn initialize_logger() -> Result<(), Box<dyn Error>> {
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
    // let video_metadata = VideoMetadata::create_and_load_from_pk(&client, 1638184921).await?;
    // info!("got video_metadata by id: {:?}", video_metadata);

    let video_metadata = VideoMetadata::select()
        .with_client(client.clone())
        .add_where_eq(name_of!(backed_up in VideoMetadata), Some(&true)).map_err(|e|anyhow!("{}",e))?
        .set_limit(10)
        .build_query()
        .map_err(|e| anyhow!("{}", e))?
        .run()
        .await
        .map_err(|e| anyhow!("{}", e))?
        .map_err_with_data("Select has to return data")
        .map_err(|e| anyhow!("{}", e))?;
    print_vec_sample("got video_metadata by backed_up:", video_metadata);

    let watched_streamers = Streamers::select()
        .with_client(client.clone())
        .add_where_eq(name_of!(watched in Streamers), Some(&true)).map_err(|e|anyhow!("{}",e))?
        .set_limit(100)
        .build_query()
        .map_err(|e| anyhow!("{}", e))?
        .run()
        .await
        .map_err(|e| anyhow!("{}", e))?
        .map_err_with_data("Select has to return data")
        .map_err(|e| anyhow!("{}", e))?;
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
