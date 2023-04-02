#![allow(unused, incomplete_features)]

use std::error::Error;
use std::io::stdin;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use chrono::{Datelike, Duration};
use downloader_config;
use downloader_config::Config;
use google_bigquery::{BigDataTable, BigqueryClient};
use google_youtube::{scopes, PrivacyStatus, YoutubeClient};
use nameof::name_of;
use tokio::io::BufReader;
use tokio::process::Command;
use twitch_data::{TwitchClient, Video};

use crate::data::{Streamers, VideoData};

pub mod data;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

async fn check_for_new_videos<'a>(
    db_client: &BigqueryClient,
    twitch_client: &TwitchClient<'a>,
) -> Result<()> {
    //check for new videos from the channels in the database that are watched
    let watched = get_watched_streamers(db_client).await?;

    println!("Got {} watched streamers", watched.len());
    //put those videos in the database if they are not already there
    for streamer in watched {
        let videos = get_twitch_videos_from_streamer(&streamer, &twitch_client).await?;
        println!("Got {} videos for {}", videos.len(), streamer.login);
        for video in videos {
            let video_id = video.id.parse()?;
            let loaded_video = data::Videos::load_from_pk(db_client, video_id).await?;
            if loaded_video.is_none() {
                let video = data::VideoData::from_twitch_video(&video, &db_client)?;
                println!(
                    "Video {} is not in the database, adding it: {}",
                    video_id,
                    video
                        .video
                        .title
                        .as_ref()
                        .unwrap_or(&"TITLE NOT FOUND".to_string())
                );
                video.video.save_to_bigquery().await?;
                video.metadata.save_to_bigquery().await?;
            }
        }
    }
    Ok(())
}

async fn get_twitch_videos_from_streamer<'a>(
    streamer: &'a Streamers<'a>,
    twitch_client: &TwitchClient<'a>,
) -> Result<Vec<Video>> {
    let videos = twitch_client
        .get_videos_from_login(&streamer.login, None)
        .await?;

    // todo!()
    Ok(videos)
}

async fn get_watched_streamers(client: &BigqueryClient) -> Result<Vec<Streamers>> {
    let watched =
        Streamers::load_by_field(client, name_of!(watched in Streamers), Some(true), 1000).await?;
    Ok(watched)
}

pub async fn start_backup() -> Result<()> {
    println!("Starting backup");
    let config = downloader_config::load_config();
    let project_id = &config.bigquery_project_id;
    let service_account_path = &config.bigquery_service_account_path;
    let dataset_id = &config.bigquery_dataset_id;
    let youtube_client_secret = &config.youtube_client_secret_path.as_str();

    let client = BigqueryClient::new(project_id, dataset_id, Some(service_account_path)).await?;
    let twitch_client = twitch_data::get_client().await?;
    println!("Starting main loop");
    'main_loop: loop {
        println!("Beginning of main loop");

        println!("Checking for new videos");
        // check_for_new_videos(&client, &twitch_client).await?;
        println!("backing up not downloaded videos");
        backup_not_downloaded_videos(&client, &twitch_client, &config).await?;

        //sleep for an hour
        println!("Sleeping for an hour");
        tokio::time::sleep(std::time::Duration::from_secs(60 * 60)).await;
        //repeat
    }
}

async fn get_not_downloaded_videos_from_db(
    client: &BigqueryClient,
) -> Result<Vec<data::VideoData>> {
    println!("getting not downloaded videos from db (metadata)");
    // let mut video_metadata_list = data::VideoMetadata::load_by_field(
    //     &client,
    //     name_of!(video_id in data::VideoMetadata),
    //     Some(1736394548),
    //     1000,
    // )
    // .await?;
    let mut video_metadata_list = data::VideoMetadata::load_by_field(
        &client,
        name_of!(backed_up in data::VideoMetadata),
        Some(false),
        1000,
    )
    .await?;
    println!("getting not downloaded videos from db (videos)");
    let amount = video_metadata_list.len();
    let mut res = vec![];
    for (i, metadata) in video_metadata_list.into_iter().enumerate() {
        println!(
            "getting not downloaded videos from db (metadata): {}/{}",
            i + 1,
            amount
        );
        let v = data::Videos::load_from_pk(client, metadata.video_id).await?;

        // let v = data::Videos::load_from_pk(client, 1744977195).await?;
        if let Some(video) = v {
            // println!("Video: {:?}", video.title);
            // println!("date: {:?}", video.created_at);
            println!(
                "getting not downloaded videos from db (streamer): {}/{}",
                i + 1,
                amount
            );
            let streamer = data::Streamers::load_from_pk(client, video.user_login.clone().unwrap())
                .await?
                .expect(
                    format!(
                        "Streamer with login not found: {}",
                        video.user_login.clone().unwrap()
                    )
                    .as_str(),
                );

            res.push(VideoData {
                video,
                metadata,
                streamer,
            });
        }
    }
    println!("Got {} videos", res.len());
    println!("Videos: {:?}", res);
    Ok(res)
}

async fn backup_not_downloaded_videos<'a>(
    client: &BigqueryClient,
    twitch_client: &TwitchClient<'a>,
    config: &Config,
) -> Result<()> {
    let path = Path::new(&config.download_folder_path);
    println!("Getting not downloaded videos from db");
    let videos = get_not_downloaded_videos_from_db(client).await?;
    println!("Got {} videos", videos.len());
    for mut video in videos {
        println!(
            "Backing up video {}: {}\nLength: {}",
            video.video.video_id,
            video.video.title.as_ref().unwrap(),
            video.video.duration.as_ref().unwrap()
        );
        let video_file_path = twitch_client
            .download_video(video.video.video_id.to_string(), "", path)
            .await?;

        let mut video_parts = split_video_into_parts(
            video_file_path.to_path_buf(),
            Duration::minutes(config.youtube_video_length_minutes),
        )
        .await?;
        video_parts.sort();

        let youtube_client = YoutubeClient::new(
            Some(config.youtube_client_secret_path.as_str()),
            vec![
                scopes::YOUTUBE_UPLOAD,
                scopes::YOUTUBE_READONLY,
                scopes::YOUTUBE,
            ],
            Some(
                video
                    .streamer
                    .youtube_user
                    .clone()
                    .unwrap_or("NopixelVODS".to_string())
                    .as_str(),
            ),
        )
        .await?;
        println!("Uploading video to youtube");
        let res = upload_video_to_youtube(&video_parts, &mut video, &youtube_client, config).await;
        if let Err(e) = res {
            println!("Error uploading video: {}", e);
            video.metadata.error = Some(e.to_string());
            video.metadata.save_to_bigquery().await?;
        } else {
            println!(
                "Video uploaded successfully: {}: {}",
                video.video.video_id,
                video.video.title.as_ref().unwrap()
            );
            video.metadata.backed_up = Some(true);
            video.metadata.save_to_bigquery().await?;
        }
        println!("Cleaning up video parts");
        cleanup_video_parts(video_parts).await?;
        println!("Video backed up");
    }

    println!("Backing up not downloaded videos finished");
    Ok(())
}
async fn cleanup_video_parts(video_parts: Vec<PathBuf>) -> Result<()> {
    for part in video_parts {
        std::fs::remove_file(part)?;
    }
    Ok(())
}

async fn upload_video_to_youtube<'a>(
    video_path: &Vec<PathBuf>,
    mut video: &mut VideoData<'a>,
    youtube_client: &YoutubeClient,
    config: &Config,
) -> Result<()> {
    let part_count = video_path.len();
    println!("Video has {} parts", part_count);
    for (i, path) in video_path.iter().enumerate() {
        println!("Uploading part {} of {}", i + 1, part_count);
        let title = get_video_title_from_twitch_video(&video, i, part_count)?;
        println!("youtube part Title: {}", title);
        let description = get_video_description_from_twitch_video(&video, i, part_count, &config)?;

        let privacy = match video.streamer.public_videos_default {
            Some(true) => PrivacyStatus::Public,
            _ => PrivacyStatus::Private,
        };
        println!("Uploading video: {}", title);
        println!("Description: {}", description);
        println!(
            "Privacy: {:?}",
            match privacy {
                PrivacyStatus::Public => "Public",
                PrivacyStatus::Private => "Private",
                PrivacyStatus::Unlisted => "Unlisted",
            }
        );

        let youtube_video = youtube_client
            .upload_video(
                path,
                &title,
                &description,
                config.youtube_tags.clone(),
                privacy,
            )
            .await?;

        let playlist_title = get_playlist_title_from_twitch_video(&video)?;
        let playlist = youtube_client
            .find_playlist_or_create_by_name(&playlist_title)
            .await?;
        youtube_client
            .add_video_to_playlist(&youtube_video, &playlist)
            .await?;
        video.metadata.youtube_playlist_url = playlist.id;
    }

    Ok(())
}

async fn split_video_into_parts(path: PathBuf, max_duration: Duration) -> Result<Vec<PathBuf>> {
    let filepath = path.canonicalize()?;
    println!(
        "Splitting video: {:?}\n\tinto parts with max duration: {} minutes",
        filepath,
        max_duration.num_minutes()
    );
    let output_path_pattern = format!("{}_%03d.mp4", filepath.to_str().unwrap()); //TODO: maybe make the number of digits dynamic
    let duration_str = duration_to_string(&max_duration);
    //example: ffmpeg -i input.mp4 -c copy -map 0 -segment_time 00:20:00 -f segment output%03d.mp4
    Command::new("ffmpeg")
        .args([
            "-i",
            filepath.to_str().unwrap(),
            "-c",
            "copy",
            "-map",
            "0",
            "-segment_time",
            &duration_str,
            "-f",
            "segment",
            &output_path_pattern,
        ])
        .output()
        .await?;

    let mut res = vec![];
    let parent_dir = path.parent().unwrap();
    let read = std::fs::read_dir(parent_dir)?;
    println!("Reading dir: {:?}", parent_dir);
    for x in read {
        // println!("Checking file: {:?}", x);
        let path = x?.path();
        if path.is_file() {
            let file_name = path.canonicalize()?;
            // let file_name = path.to_str().unwrap();
            println!("Checking file: {:?}", file_name);
            let filename_beginning_pattern = format!("{}_", &filepath.to_str().unwrap());
            let filename_str = file_name.to_str().unwrap();
            if filename_str.starts_with(&filename_beginning_pattern)
                && filename_str.ends_with(".mp4")
            {
                println!("Found file: {:?}", file_name);
                res.push(path);
            } else {
                println!("Skipping file:       {:?}", file_name);
                println!("Filepath to compare: {:?}", filename_beginning_pattern);
                println!(
                    "Starts with: {}",
                    filename_str.starts_with(&filename_beginning_pattern)
                );
                println!("Ends with: {}", filename_str.ends_with(".mp4"));
            }
        }
    }
    tokio::fs::remove_file(&path).await?;
    println!("Split video into {} parts", res.len());
    // println!("Video parts: {:?}", res);
    // stdin().read_line(&mut String::new()).unwrap();
    Ok(res)
}

//region get title stuff
/// get the description for the video with the template from the config
///
/// possible variables:
/// - $$video_title$$
/// - $$video_url$$
/// - $$video_id$$
/// - $$video_duration$$
/// - $$video_part$$
/// - $$video_total_parts$$
/// - $$video_streamer_name$$
/// - $$video_streamer_login$$
pub fn get_video_description_from_twitch_video(
    video: &data::VideoData,
    part: usize,
    total_parts: usize,
    config: &Config,
) -> Result<String> {
    let description_template = &config.youtube_description_template;
    let description = description_template.clone();

    let description = description.replace("$$video_title$$", &video.video.title.as_ref().unwrap());

    let url = &video
        .video
        .url
        .clone()
        .unwrap_or("<NO URL FOUND>".to_string());
    let description = description.replace("$$video_url$$", url);

    let description = description.replace("$$video_id$$", &video.video.video_id.to_string());

    let duration = &video.video.duration.unwrap_or(0);
    let duration = chrono::Duration::seconds(duration.clone());
    let duration = duration.to_string();
    let description = description.replace("$$video_duration$$", &duration);

    let description = description.replace("$$video_part$$", &part.to_string());

    let description = description.replace("$$video_total_parts$$", &total_parts.to_string());

    let description = description.replace(
        "$$video_streamer_name$$",
        &video
            .streamer
            .display_name
            .clone()
            .unwrap_or("<NO NAME FOUND>".to_string()),
    );

    let description =
        description.replace("$$video_streamer_login$$", &video.streamer.login.clone());

    Ok(description)
}
pub fn get_video_title_from_twitch_video(
    video: &data::VideoData,
    part: usize,
    total_parts: usize,
) -> Result<String> {
    let prefix = match total_parts {
        1 => "".to_string(),
        _ => format!(
            "{} ",
            get_video_prefix_from_twitch_video(video, part, total_parts)?
        ),
    };
    let title = get_playlist_title_from_twitch_video(video)?;

    let res = format!("{}{}", prefix, title);
    Ok(res)
}

const MAX_VIDEO_TITLE_LENGTH: usize = 100;
const PREFIX_LENGTH: usize = 24;

pub fn get_playlist_title_from_twitch_video(video: &data::VideoData) -> Result<String> {
    let title = video.video.title.as_ref().ok_or("Video has no title")?;
    const SEPARATOR_LEN: usize = 1;
    if title.len() > MAX_VIDEO_TITLE_LENGTH - PREFIX_LENGTH - SEPARATOR_LEN {
        let res = format!(
            "{}...",
            &title[0..MAX_VIDEO_TITLE_LENGTH - PREFIX_LENGTH - SEPARATOR_LEN - 3]
        );
        return Ok(res);
    }
    Ok(title.to_string())
}

pub fn get_video_prefix_from_twitch_video(
    video: &data::VideoData,
    part: usize,
    total_parts: usize,
) -> Result<String> {
    println!("video: {:?}", video);
    println!("video.video: {:?}", video.video);
    println!("video.video.created_at: {:?}", video.video.created_at);
    let created_at = video
        .video
        .created_at
        .ok_or(format!("Video has no created_at time: {:?}", video.video).as_str())?;
    // let created_at = created_at.format("%Y-%m-%d");
    let res = format!(
        "[{:0>4}-{:0>2}-{:0>2}][Part {:0>2}/{:0>2}]",
        created_at.year(),
        created_at.month(),
        created_at.day(),
        part,
        total_parts
    );
    Ok(res)
}

//endregion

fn duration_to_string(duration: &Duration) -> String {
    let seconds = duration.num_seconds();
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;
    format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
}
