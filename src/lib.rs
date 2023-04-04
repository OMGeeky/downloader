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
use log::{debug, error, info, trace, warn};
use nameof::name_of;
use path_clean::clean;
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
    trace!("Checking for new videos");
    //check for new videos from the channels in the database that are watched
    let watched = get_watched_streamers(db_client).await?;

    info!("Got {} watched streamers", watched.len());
    //put those videos in the database if they are not already there
    for streamer in watched {
        let videos = get_twitch_videos_from_streamer(&streamer, &twitch_client).await?;
        info!("Got {} videos for {}", videos.len(), streamer.login);
        for video in videos {
            let video_id = video.id.parse()?;
            let loaded_video = data::Videos::load_from_pk(db_client, video_id).await?;
            if loaded_video.is_none() {
                let video = data::VideoData::from_twitch_video(&video, &db_client)?;
                info!(
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
    trace!("Getting videos from streamer {}", streamer.login);
    let videos = twitch_client
        .get_videos_from_login(&streamer.login, None)
        .await?;

    Ok(videos)
}

async fn get_watched_streamers(client: &BigqueryClient) -> Result<Vec<Streamers>> {
    trace!("Getting watched streamers");
    let watched =
        Streamers::load_by_field(client, name_of!(watched in Streamers), Some(true), 1000).await?;
    Ok(watched)
}

pub async fn start_backup() -> Result<()> {
    info!("Starting backup");
    let config = downloader_config::load_config();
    info!("loaded config");
    let project_id = &config.bigquery_project_id;
    let service_account_path = &config.bigquery_service_account_path;
    let dataset_id = &config.bigquery_dataset_id;
    let youtube_client_secret = &config.youtube_client_secret_path.as_str();

    info!("creating BigqueryClient");
    let client = BigqueryClient::new(project_id, dataset_id, Some(service_account_path)).await?;
    info!("creating twitch client");
    let twitch_client = twitch_data::get_client().await?;
    info!("Starting main loop");
    'main_loop: loop {
        trace!("Beginning of main loop");

        trace!("Checking for new videos");
        check_for_new_videos(&client, &twitch_client).await?;
        trace!("backing up not downloaded videos");
        backup_not_downloaded_videos(&client, &twitch_client, &config).await?;

        //sleep for an hour
        info!("Sleeping for an hour");
        tokio::time::sleep(std::time::Duration::from_secs(60 * 60)).await;
        //repeat
    }
}

async fn get_not_downloaded_videos_from_db(
    client: &BigqueryClient,
) -> Result<Vec<data::VideoData>> {
    info!("getting not downloaded videos from db (metadata)");
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
    info!("getting not downloaded videos from db (videos)");
    let amount = video_metadata_list.len();
    let mut res = vec![];
    for (i, metadata) in video_metadata_list.into_iter().enumerate() {
        info!(
            "getting not downloaded videos from db (metadata): {}/{}",
            i + 1,
            amount
        );
        let v = data::Videos::load_from_pk(client, metadata.video_id).await?;

        // let v = data::Videos::load_from_pk(client, 1744977195).await?;
        if let Some(video) = v {
            // info!("Video: {:?}", video.title);
            // info!("date: {:?}", video.created_at);
            info!(
                "getting not downloaded videos from db (streamer): {}/{}",
                i + 1,
                amount
            );
            let user_login = video.user_login.clone().unwrap().to_lowercase();
            let streamer = data::Streamers::load_from_pk(client, user_login.clone()).await?;
            if streamer.is_none() {
                // .expect(format!("Streamer with login not found: {}", user_login).as_str());
                warn!("Streamer with login not found: {}", user_login);
                continue;
            }
            let streamer = streamer.unwrap();

            res.push(VideoData {
                video,
                metadata,
                streamer,
            });
        }
    }
    info!("Got {} videos", res.len());
    info!("Videos: {:?}", res);
    Ok(res)
}

async fn backup_not_downloaded_videos<'a>(
    client: &BigqueryClient,
    twitch_client: &TwitchClient<'a>,
    config: &Config,
) -> Result<()> {
    trace!("backup not downloaded videos");
    let path = Path::new(&config.download_folder_path);
    info!("Getting not downloaded videos from db");
    let videos = get_not_downloaded_videos_from_db(client).await?;
    info!("Got {} videos", videos.len());
    for mut video in videos {
        info!(
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
            Duration::minutes(config.youtube_video_length_minutes_soft_cap),
            Duration::minutes(config.youtube_video_length_minutes_hard_cap),
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
        info!("Uploading video to youtube");
        let res = upload_video_to_youtube(&video_parts, &mut video, &youtube_client, config).await;
        if let Err(e) = res {
            info!("Error uploading video: {}", e);
            video.metadata.error = Some(e.to_string());
            video.metadata.save_to_bigquery().await?;
        } else {
            info!(
                "Video uploaded successfully: {}: {}",
                video.video.video_id,
                video.video.title.as_ref().unwrap()
            );
            video.metadata.backed_up = Some(true);
            video.metadata.save_to_bigquery().await?;
        }
        info!("Cleaning up video parts");
        cleanup_video_parts(video_parts).await?;
        info!("Video backed up");
    }

    info!("Backing up not downloaded videos finished");
    Ok(())
}
async fn cleanup_video_parts(video_parts: Vec<PathBuf>) -> Result<()> {
    trace!("cleanup video parts");
    for part in video_parts {
        trace!("Removing part: {}", part.display());
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
    trace!("upload video to youtube");
    let part_count = video_path.len();
    info!("Video has {} parts", part_count);
    for (i, path) in video_path.iter().enumerate() {
        info!("Uploading part {} of {}", i + 1, part_count);
        let title = get_video_title_from_twitch_video(&video, i, part_count)?;
        info!("youtube part Title: {}", title);
        let description = get_video_description_from_twitch_video(&video, i, part_count, &config)?;

        let privacy = match video.streamer.public_videos_default {
            Some(true) => PrivacyStatus::Public,
            _ => PrivacyStatus::Private,
        };
        info!("Uploading video: {}", title);
        info!("Description: {}", description);
        info!(
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

pub async fn split_video_into_parts(
    path: PathBuf,
    duration_soft_cap: Duration,
    duration_hard_cap: Duration,
) -> Result<Vec<PathBuf>> {
    trace!("split video into parts");
    //region prepare paths
    let filepath = path.canonicalize()?;
    let parent_dir = path.parent().unwrap().canonicalize();
    if parent_dir.is_err() {
        warn!("Could not canonicalize parent dir");
    }
    let parent_dir = parent_dir.expect("Could not canonicalize parent dir");

    let file_playlist = clean(Path::join(&parent_dir, "output.m3u8"));
    //endregion
    info!(
        "Splitting video: {:?}\n\tinto parts with soft cap duration: {} minutes and hard cap duration: {} minutes",
        filepath,
        duration_soft_cap.num_minutes(),
        duration_hard_cap.num_minutes()
    );

    let output_path_pattern = format!("{}_%03d.mp4", filepath.to_str().unwrap()); //TODO: maybe make the number of digits dynamic
    let duration_str = duration_to_string(&duration_soft_cap);

    //region run ffmpeg split command
    //example: ffmpeg -i input.mp4 -c copy -map 0 -segment_time 00:20:00 -f segment output%03d.mp4
    trace!(
        "Running ffmpeg command: ffmpeg -i {:?} -c copy -map 0 -segment_time {} -reset_timestamps 1\
         -segment_list {} -segment_list_type m3u8 -avoid_negative_ts 1 -f segment {}",
        filepath,
        duration_str,
        file_playlist.display(),
        output_path_pattern
    );
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
            "-reset_timestamps",
            "1",
            "-segment_list",
            file_playlist.to_str().unwrap(),
            "-segment_list_type",
            "m3u8",
            "-avoid_negative_ts",
            "1",
            "-f",
            "segment",
            &output_path_pattern,
        ])
        .output()
        .await?;
    trace!("Finished running ffmpeg command");
    //endregion

    //region extract parts from playlist file (create by ffmpeg 'output.m3u8')
    let mut res = vec![];
    info!("Reading playlist file: {}", file_playlist.display());
    let playlist = tokio::fs::read_to_string(&file_playlist).await;
    if playlist.is_err() {
        warn!("Failed to read playlist file: {}", file_playlist.display());
    }
    let playlist =
        playlist.expect(format!("Failed to read playlist {}", file_playlist.display()).as_str());
    let mut last_time = 0.0;
    let mut time = 0.0;
    let mut last_path: Option<PathBuf> = None;
    let mut current_path: Option<PathBuf> = None;
    for line in playlist.lines() {
        if line.starts_with("#") {
            if line.starts_with("#EXTINF:") {
                last_time = time;
                time = line["#EXTINF:".len()..].parse::<f64>().unwrap_or(0.0);
            }
            continue;
        }
        last_path = current_path;
        current_path = Some(Path::join(&parent_dir, line));
        res.push(current_path.clone().unwrap());
    }
    //endregion

    //region maybe join last two parts
    trace!("Deciding if last two parts should be joined");
    if let Some(last_path) = last_path {
        if let Some(current_path) = current_path {
            let joined_time = last_time + time;
            if joined_time < duration_soft_cap.num_seconds() as f64 {
                //region join last two parts
                info!("Joining last two parts");

                //remove the part from the result that is going to be joined
                res.pop();

                let join_txt_path = Path::join(&parent_dir, "join.txt");
                let join_mp4_path = Path::join(&parent_dir, "join.mp4");
                tokio::fs::write(
                    join_txt_path.clone(),
                    format!(
                        "file '{}'\nfile '{}'",
                        clean(&last_path)
                            .to_str()
                            .expect("to_str on path did not work!"),
                        clean(&current_path)
                            .to_str()
                            .expect("to_str on path did not work!")
                    ),
                )
                .await?;

                // example: ffmpeg -f concat -safe 0 -i join.txt -c copy joined.mp4
                // content of join.txt:
                // file 'output_002.mp4'
                // file 'output_003.mp4'
                let join_txt_path = clean(join_txt_path);
                let join_mp4_path = clean(join_mp4_path);

                trace!(
                    "Running ffmpeg command: ffmpeg -f concat -safe 0 -i {:?} -c copy {:?}",
                    join_txt_path,
                    join_mp4_path
                );
                Command::new("ffmpeg")
                    .args([
                        "-f",
                        "concat",
                        "-safe",
                        "0",
                        "-i",
                        join_txt_path
                            .to_str()
                            .expect("to_str on join_txt_path did not work!"),
                        "-c",
                        "copy",
                        join_mp4_path
                            .to_str()
                            .expect("to_str on join_mp4_path did not work!"),
                    ])
                    .output()
                    .await?;
                trace!("Finished running ffmpeg command");
                //region remove files
                trace!(
                    "Removing files: {:?}, {:?}, {:?} {:?}",
                    current_path,
                    last_path,
                    join_txt_path,
                    file_playlist,
                );
                tokio::fs::remove_file(current_path).await?;
                tokio::fs::remove_file(&last_path).await?;
                tokio::fs::remove_file(join_txt_path).await?;
                tokio::fs::remove_file(file_playlist).await?;
                //endregion
                trace!("Renaming file: {:?} to {:?}", join_mp4_path, last_path);
                tokio::fs::rename(join_mp4_path, last_path).await?;
                info!("Joined last two parts");
                //endregion
            }
        }
    }
    //endregion

    info!("removing the original file");
    tokio::fs::remove_file(&path).await?;

    info!("Split video into {} parts", res.len());
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
    trace!("get video description from twitch video");
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
    trace!("get video title from twitch video");
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
    trace!("get playlist title from twitch video");
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
    trace!("get video prefix from twitch video");
    info!("video: {:?}", video);
    info!("video.video: {:?}", video.video);
    info!("video.video.created_at: {:?}", video.video.created_at);
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
    trace!("duration to string for duration: {:?}", duration);
    let seconds = duration.num_seconds();
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;
    format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
}
