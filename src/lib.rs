#![allow(unused, incomplete_features)]

use std::error::Error;
use std::future::Future;
use std::io::stdin;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{anyhow, Result};
use chrono::{Datelike, Duration};
use downloader_config;
use downloader_config::Config;
use google_bigquery_v2::prelude::*;
use google_youtube::{scopes, PrivacyStatus, YoutubeClient};
use log::{debug, error, info, trace, warn};
use nameof::name_of;
use path_clean::clean;
use tokio::io::BufReader;
use tokio::process::Command;
use twitch_data::{TwitchClient, Video};

use crate::data::{Streamers, VideoData};
use crate::prelude::*;

pub mod data;
pub mod prelude;

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
            let video_id: &i64 = &video.id.parse()?;
            let loaded_video = data::Videos::get_by_pk(db_client.clone(), video_id).await;
            if loaded_video.is_err() {
                let mut video = data::VideoData::from_twitch_video(&video, db_client.clone())
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                info!(
                    "Video {} is not in the database, adding it: {}",
                    video_id,
                    video
                        .video
                        .title
                        .as_ref()
                        .unwrap_or(&"TITLE NOT FOUND".to_string())
                );
                video.video.save().await.map_err(|e| anyhow!("{}", e))?;
                video.metadata.save().await;
            }
        }
    }
    Ok(())
}

async fn get_twitch_videos_from_streamer<'a>(
    streamer: &Streamers,
    twitch_client: &TwitchClient<'a>,
) -> Result<Vec<Video>> {
    trace!("Getting videos from streamer {}", streamer.login);
    let videos = twitch_client
        .get_videos_from_login(&streamer.login, None)
        .await
        .map_err(|e| anyhow!("{}", e))?;

    Ok(videos)
}

async fn get_watched_streamers(client: &BigqueryClient) -> Result<Vec<Streamers>> {
    trace!("Getting watched streamers");
    let watched = Streamers::select()
        .with_client(client.clone())
        .add_where_eq(name_of!(watched in Streamers), Some(&true))
        .map_err(|e| anyhow!("{}", e))?
        .set_limit(1000)
        .build_query()
        .map_err(|e| anyhow!("{}", e))?
        .run()
        .await
        .map_err(|e| anyhow!("{}", e))?;
    let watched = watched
        .map_err_with_data("Error getting watched streamers")
        .map_err(|e| anyhow!("{}", e))?;
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
    let client = BigqueryClient::new(project_id, dataset_id, Some(service_account_path))
        .await
        .map_err(|e| anyhow!("{}", e))?;
    info!("creating twitch client");
    let twitch_client = twitch_data::get_client()
        .await
        .map_err(|e| anyhow!("{}", e))?;
    info!("Starting main loop");
    'main_loop: loop {
        trace!("Beginning of main loop");

        trace!("Checking for new videos");
        check_for_new_videos(&client, &twitch_client).await?;
        trace!("backing up not downloaded videos");
        backup_not_downloaded_videos(&client, &twitch_client, &config)
            .await
            .map_err(|e| anyhow!("{}", e))?;

        //sleep for an hour
        info!("Sleeping for an hour");
        tokio::time::sleep(std::time::Duration::from_secs(60 * 60)).await;
        //repeat
    }
}

async fn get_not_downloaded_videos_from_db(
    client: &BigqueryClient,
) -> Result<impl Iterator<Item = impl Future<Output = Result<Option<data::VideoData>>> + '_> + '_> {
    //TODO: make sure that this is sorted by date (oldest first)
    info!("getting not downloaded videos from db (metadata)");

    let mut video_metadata_list = data::VideoMetadata::select()
        .with_client(client.clone())
        .add_where_eq(name_of!(backed_up in data::VideoMetadata), Some(&false))
        .map_err(|e| anyhow!("{}", e))?
        .add_order_by(
            name_of!(video_id in data::VideoMetadata),
            OrderDirection::Ascending,
        )
        //TODO: check if ordering by video_id is correct (should be oldest first)
        //TODO: sort this by streamer (join is needed)
        .set_limit(1000)
        .build_query()
        .map_err(|e| anyhow!("{}", e))?
        .run()
        .await
        .map_err(|e| anyhow!("{}", e))?
        .map_err_with_data("Error getting not downloaded videos from db")
        .map_err(|e| anyhow!("{}", e))?;
    info!("getting not downloaded videos from db (videos)");
    let amount = video_metadata_list.len();
    info!("got about {} videos", amount);
    let res = video_metadata_list
        .into_iter()
        .enumerate()
        .map(move |(i, metadata)| async move {
            info!(
                "getting not downloaded videos from db (metadata): {}/{}",
                i + 1,
                amount
            );
            let video_id = metadata.video_id.clone();
            let v = data::Videos::get_by_pk(client.clone(), &video_id.clone()).await;

            if let Ok(video) = v {
                info!(
                    "getting not downloaded videos from db (streamer): {}/{}",
                    i + 1,
                    amount
                );
                let user_login = video.user_login.clone().unwrap().to_lowercase();
                let streamer =
                    data::Streamers::get_by_pk(client.clone(), &user_login.clone()).await;
                if streamer.is_ok() {
                    // .expect(format!("Streamer with login not found: {}", user_login).as_str());
                    warn!("Streamer with login not found: {}", user_login);
                    return Ok(None);
                }
                let streamer = streamer.unwrap();

                return Ok(Some(VideoData {
                    video,
                    metadata,
                    streamer,
                }));
            }
            Ok(None)
        }); //TODO: maybe figure out how to use the filter method on this async iterator (filter out None values)
    return Ok(res);
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
    for mut video in videos.into_iter() {
        let video = video.await?;

        if video.is_none() {
            continue;
        }
        let mut video = video.unwrap();

        let result = backup_video(twitch_client, config, path, &mut video).await;
        if result.is_err() {
            let e = result.unwrap_err();
            let error_message = format!("Error while backing up video: {}", e.to_string());
            warn!("{}", error_message);
            video.metadata.error = Some(error_message);
            video.metadata.backed_up = Some(false);
            video.metadata.save().await.map_err(|e| anyhow!("{}", e))?;
            continue;
        }
        info!("Video backed up");
    }

    info!("Backing up not downloaded videos finished");
    Ok(())
}

async fn backup_video<'a>(
    twitch_client: &TwitchClient<'a>,
    config: &Config,
    path: &Path,
    video: &mut VideoData,
) -> Result<()> {
    info!(
        "Backing up video {}: {}\nLength: {}",
        video.video.video_id,
        video.video.title.as_ref().unwrap(),
        video.video.duration.as_ref().unwrap()
    );
    let video_file_path = twitch_client
        .download_video(video.video.video_id.to_string(), "", path)
        .await;
    if video_file_path.is_err() {
        warn!(
            "Failed to download video: {}: {:?}",
            video.video.video_id, video.video.title
        );
        return Err(anyhow!(
            "Failed to download video: {}: {:?}",
            video.video.video_id,
            video.video.title
        ));
    }
    let video_file_path = video_file_path.unwrap();
    info!("Splitting video into parts");
    //TODO: optimization: if the video is shorter than the soft cap, then skip this step
    let mut video_parts = split_video_into_parts(
        video_file_path.to_path_buf(),
        Duration::minutes(config.youtube_video_length_minutes_soft_cap),
        Duration::minutes(config.youtube_video_length_minutes_hard_cap),
    )
    .await?;
    video_parts.sort();
    trace!("Creating youtube client");
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
    .await
    .map_err(|e| anyhow!("{}", e))?;
    info!("Uploading video to youtube");
    debug!("Video parts: {:?}", video_parts);
    debug!("Video: {:?}", video);
    debug!("Config: {:?}", config);
    let res = upload_video_to_youtube(&video_parts, video, &youtube_client, config).await;
    if let Err(e) = res {
        info!("Error uploading video: {}", e);
        video.metadata.error = Some(e.to_string());
        video.metadata.save().await.map_err(|e| anyhow!("{}", e))?;
    } else {
        info!(
            "Video uploaded successfully: {}: {}",
            video.video.video_id,
            video.video.title.as_ref().unwrap()
        );
        video.metadata.backed_up = Some(true);
        video.metadata.save().await.map_err(|e| anyhow!("{}", e))?;
    }
    info!("Cleaning up video parts");
    cleanup_video_parts(video_parts).await?;
    info!("Video backed up");
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
    mut video: &mut VideoData,
    youtube_client: &YoutubeClient,
    config: &Config,
) -> Result<()> {
    trace!("upload video to youtube");
    let part_count = video_path.len();
    info!("Video has {} parts", part_count);
    for (i, path) in video_path.iter().enumerate() {
        info!("Uploading part {} of {}", i + 1, part_count);
        let title = get_video_title_from_twitch_video(&video, i + 1, part_count)?;
        info!("youtube part Title: {}", title);
        let description =
            get_video_description_from_twitch_video(&video, i + 1, part_count, &config)?;

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
            .await
            .map_err(|e| anyhow!("{}", e))?;

        let playlist_title = get_playlist_title_from_twitch_video(&video)?;
        let playlist = youtube_client
            .find_playlist_or_create_by_name(&playlist_title)
            .await
            .map_err(|e| anyhow!("{}", e))?;
        youtube_client
            .add_video_to_playlist(&youtube_video, &playlist)
            .await
            .map_err(|e| anyhow!("{}", e))?;
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
        "Splitting video: {:?} into parts with soft cap duration: {} minutes and hard cap duration: {} minutes",
        filepath,
        duration_soft_cap.num_minutes(),
        duration_hard_cap.num_minutes()
    );

    let output_path_pattern = Path::join(
        &parent_dir,
        format!(
            "{}_%03d.mp4",
            filepath
                .file_stem()
                .expect("could not get file_stem from path")
                .to_str()
                .expect("could not convert file_stem to str")
        ),
    )
    .to_str()
    .expect("could not convert path to string")
    .to_string(); //TODO: maybe make the number of digits dynamic
    debug!("output path pattern: {}", output_path_pattern);
    let duration_str = duration_to_string(&duration_soft_cap);

    //region run ffmpeg split command
    //example: ffmpeg -i input.mp4 -c copy -map 0 -segment_time 00:20:00 -f segment output%03d.mp4
    debug!(
        "Running ffmpeg command: ffmpeg -i {:?} -c copy -map 0 -segment_time {} -reset_timestamps 1 \
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
    debug!("Finished running ffmpeg command");
    //endregion

    //region extract parts from playlist file (create by ffmpeg 'output.m3u8')
    let (mut paths, second_last_time, last_time, second_last_path, last_path) =
        extract_track_info_from_playlist_file(&parent_dir, &file_playlist).await?;
    //endregion

    //region maybe join last two parts
    debug!("Deciding if last two parts should be joined");
    if let Some(second_last_path) = second_last_path {
        if let Some(last_path) = last_path {
            let joined_time = second_last_time + last_time;
            let general_info = format!("second last part duration: {} seconds, \
                    last part duration: {} seconds, joined duration: {} seconds (hard cap: {} seconds)",
                    second_last_time, last_time, joined_time, duration_hard_cap.num_seconds());
            if joined_time < duration_hard_cap.num_seconds() as f64 {
                //region join last two parts
                info!("Joining last two parts. {}", general_info);

                //remove the part from the result that is going to be joined
                paths.pop();

                let join_txt_path = Path::join(&parent_dir, "join.txt");
                let join_mp4_path = Path::join(&parent_dir, "join.mp4");
                let second_last_path = clean(&second_last_path);
                let second_last_path_str = second_last_path
                    .to_str()
                    .expect("to_str on path did not work!");
                let last_path = clean(&last_path);
                let last_path = last_path.to_str().expect("to_str on path did not work!");
                tokio::fs::write(
                    join_txt_path.clone(),
                    format!("file '{}'\nfile '{}'", last_path, second_last_path_str,),
                )
                .await?;

                // example: ffmpeg -f concat -safe 0 -i join.txt -c copy joined.mp4
                // content of join.txt:
                // file 'output_002.mp4'
                // file 'output_003.mp4'
                let join_txt_path = clean(join_txt_path);
                let join_mp4_path = clean(join_mp4_path);

                debug!(
                    "Running ffmpeg command: ffmpeg -f concat -safe 0 -i {:?} -c copy {:?}",
                    join_txt_path, join_mp4_path
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
                debug!("Finished running ffmpeg command");
                //region remove files
                debug!(
                    "Removing files: {:?}, {:?}, {:?} {:?}",
                    second_last_path, last_path, join_txt_path, file_playlist,
                );
                tokio::fs::remove_file(&second_last_path).await?;
                tokio::fs::remove_file(&last_path).await?;
                tokio::fs::remove_file(join_txt_path).await?;
                tokio::fs::remove_file(file_playlist).await?;
                //endregion
                debug!(
                    "Renaming file: {:?} to {:?}",
                    join_mp4_path, second_last_path
                );
                tokio::fs::rename(join_mp4_path, second_last_path).await?;
                info!("Joined last two parts");
                //endregion
            } else {
                info!("Not joining last two parts: {}", general_info);
            }
        } else {
            warn!("second_last_path was Some but last_path was None. This should not happen!");
        }
    } else {
        warn!("second_last_path was None. This should only happen if the total length is shorter than the hard cap!");
    }
    //endregion

    info!("removing the original file");
    tokio::fs::remove_file(&path).await?;

    info!("Split video into {} parts", paths.len());
    Ok(paths)
}

pub fn extract_track_info_from_playlist(playlist: String) -> Result<(f64, Vec<(String, f64)>)> {
    let mut res = vec![];
    let mut total_time: f64 = -1.0;

    let mut last_time = None;
    for line in playlist.lines() {
        if line.starts_with("#EXTINF:") {
            let time_str = line.replace("#EXTINF:", "");
            let time_str = time_str.trim();
            let time_str = time_str.strip_suffix(",").unwrap_or(time_str);
            last_time = Some(time_str.parse::<f64>()?);
        } else if line.starts_with("#EXT-X-ENDLIST") {
            break;
        } else if line.starts_with("#EXT-X-TARGETDURATION:") {
            let time_str = line.replace("#EXT-X-TARGETDURATION:", "");
            total_time = time_str.parse::<f64>()?;
        } else if let Some(time) = last_time {
            let path = line.trim().to_string();
            res.push((path, time));
            last_time = None;
        }
    }

    Ok((total_time, res))
}

///
pub async fn extract_track_info_from_playlist_file(
    parent_dir: &PathBuf,
    file_playlist: &PathBuf,
) -> Result<(Vec<PathBuf>, f64, f64, Option<PathBuf>, Option<PathBuf>)> {
    let mut res = vec![];
    info!("Reading playlist file: {}", file_playlist.display());
    let playlist = tokio::fs::read_to_string(&file_playlist).await;
    if playlist.is_err() {
        warn!("Failed to read playlist file: {}", file_playlist.display());
    }
    let playlist = playlist?;
    let mut last_time = 0.0;
    let mut time = 0.0;
    let mut last_path: Option<PathBuf> = None;
    let mut current_path: Option<PathBuf> = None;

    let (_total, parts) = extract_track_info_from_playlist(playlist)?;

    for (path, part_time) in &parts {
        last_time = time;
        time = *part_time;
        last_path = current_path;
        current_path = Some(Path::join(parent_dir, path));
    }

    res = parts
        .iter()
        .map(|(path, _)| Path::join(parent_dir, path))
        .collect::<Vec<PathBuf>>();

    Ok((res, last_time, time, last_path, current_path))
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

    let title = video
        .video
        .title
        .as_ref()
        .ok_or("Video has no title")
        .map_err(|e| anyhow!("{}", e))?;
    let title = cap_long_title(title)?;

    let res = format!("{}{}", prefix, title);
    Ok(res)
}

const MAX_VIDEO_TITLE_LENGTH: usize = 100;
const PREFIX_LENGTH: usize = 24;

pub fn get_playlist_title_from_twitch_video(video: &data::VideoData) -> Result<String> {
    trace!("get playlist title from twitch video");
    let title = video
        .video
        .title
        .as_ref()
        .ok_or("Video has no title")
        .map_err(|e| anyhow!("{}", e))?;
    let date_str = get_date_string_from_video(video)?;
    let title = format!("{} {}", date_str, title,);
    let title = cap_long_title(title)?;
    Ok(title)
}
pub fn cap_long_title<S: Into<String>>(title: S) -> Result<String> {
    let title = title.into();
    const SEPARATOR_LEN: usize = 1;
    if title.len() > MAX_VIDEO_TITLE_LENGTH - PREFIX_LENGTH - SEPARATOR_LEN {
        let shortened = format!(
            "{}...",
            &title[0..MAX_VIDEO_TITLE_LENGTH - PREFIX_LENGTH - SEPARATOR_LEN - 3]
        );
        return Ok(shortened);
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
    let res = get_date_string_from_video(video)?;
    let res = format!("{}[Part {:0>2}/{:0>2}]", res, part, total_parts);
    Ok(res)
}

fn get_date_string_from_video(video: &VideoData) -> Result<String> {
    let created_at = video
        .video
        .created_at
        .ok_or(format!("Video has no created_at time: {:?}", video.video).as_str())
        .map_err(|e| anyhow!("{}", e))?;
    // let created_at = created_at.format("%Y-%m-%d");
    let res = format!(
        "[{:0>4}-{:0>2}-{:0>2}]",
        created_at.year(),
        created_at.month(),
        created_at.day(),
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

//region tests
#[cfg(test)]
mod tests {
    use std::fs::File;

    use tokio::io::AsyncReadExt;

    use super::*;

    #[test]
    fn test_duration_to_string() {
        let duration = Duration::seconds(0);
        let res = duration_to_string(&duration);
        assert_eq!(res, "00:00:00");

        let duration = Duration::seconds(1);
        let res = duration_to_string(&duration);
        assert_eq!(res, "00:00:01");

        let duration = Duration::seconds(60);
        let res = duration_to_string(&duration);
        assert_eq!(res, "00:01:00");

        let duration = Duration::seconds(3600);
        let res = duration_to_string(&duration);
        assert_eq!(res, "01:00:00");

        let duration = Duration::seconds(3600 + 60 + 1);
        let res = duration_to_string(&duration);
        assert_eq!(res, "01:01:01");
    }

    #[tokio::test]
    async fn test_extract_track_info_from_playlist() {
        let sample_playlist_content = tokio::fs::read_to_string("tests/test_data/playlist.m3u8")
            .await
            .unwrap();

        let (total_time, parts) = extract_track_info_from_playlist(sample_playlist_content)
            .expect("failed to extract track info from playlist");
        assert_eq!(total_time, 18002.0 as f64);
        assert_eq!(parts.len(), 2);

        assert_eq!(
            parts[0],
            ("1740252892.mp4_000.mp4".to_string(), 18001.720898 as f64)
        );
        assert_eq!(
            parts[1],
            ("1740252892.mp4_001.mp4".to_string(), 14633.040755 as f64)
        );
    }
    #[tokio::test]
    async fn test_extract_track_info_from_playlist_file() {
        let parent_dir = Path::new("tests/test_data/");
        let res = extract_track_info_from_playlist_file(
            &parent_dir.into(),
            &Path::join(parent_dir, "playlist.m3u8"),
        )
        .await
        .unwrap();
        // .expect("failed to extract track info from playlist");
        let (parts, second_last_time, last_time, second_last_path, last_path) = res;
        assert_eq!(parts.len(), 2);

        assert_eq!(
            second_last_path,
            Some(Path::join(parent_dir, "1740252892.mp4_000.mp4"))
        );
        assert_eq!(
            last_path,
            Some(Path::join(parent_dir, "1740252892.mp4_001.mp4"))
        );
        assert_eq!(parts[0], Path::join(parent_dir, "1740252892.mp4_000.mp4"));
        assert_eq!(parts[1], Path::join(parent_dir, "1740252892.mp4_001.mp4"));
        assert_eq!(second_last_time, 18001.720898 as f64);
        assert_eq!(last_time, 14633.040755 as f64);
    }
}

//endregion
