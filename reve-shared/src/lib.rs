use clap::Parser;
use indicatif::ProgressBar;
use path_clean::PathClean;
use rayon::prelude::*;
use rusqlite::{params, Connection, Result};
use serde::{Deserialize, Serialize};
use serde_json::from_str;
use serde_json::Value;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Error, ErrorKind};
use std::path::Path;
use std::process::exit;
use std::process::Output;
use std::process::{ChildStderr, Command, Stdio};
use std::str::FromStr;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::vec;
use walkdir::WalkDir;

#[derive(Serialize, Deserialize)]
pub struct Segment {
    pub index: u32,
    pub size: u32,
}

#[derive(Serialize, Deserialize)]
pub struct Video {
    pub path: String,
    pub output_path: String,
    pub segments: Vec<Segment>,
    pub frame_rate: f32,
    pub frame_count: u32,
    pub segment_size: u32,
    pub segment_count: u32,
    pub upscale_ratio: u8,
}

impl Video {
    pub fn new(path: &str, output_path: &str, segment_size: u32, upscale_ratio: u8) -> Video {
        let frame_count = {
            let output = Command::new("mediainfo")
                .arg("--Output=Video;%FrameCount%")
                .arg(path)
                .output()
                .expect("failed to execute process");
            let r = String::from_utf8(output.stdout)
                .unwrap()
                .trim()
                .parse::<u32>();
            match r {
                Err(_e) => 0,
                _ => r.unwrap(),
            }
        };

        let frame_rate = {
            let output = Command::new("mediainfo")
                .arg("--Output=Video;%FrameRate%")
                .arg(path)
                .output()
                .expect("failed to execute process");
            String::from_utf8(output.stdout)
                .unwrap()
                .trim()
                .to_string()
                .parse::<f32>()
                .unwrap()
        };

        let parts_num = (frame_count as f32 / segment_size as f32).ceil() as i32;
        let last_segment_size = get_last_segment_size(frame_count, segment_size);

        let mut segments = Vec::new();
        for i in 0..(parts_num - 1) {
            let frame_number = segment_size;
            segments.push(Segment {
                index: i as u32,
                size: frame_number as u32,
            });
        }
        segments.push(Segment {
            index: (parts_num - 1) as u32,
            size: last_segment_size as u32,
        });

        let segment_count = segments.len() as u32;

        Video {
            path: path.to_string(),
            output_path: output_path.to_string(),
            segments,
            frame_rate,
            frame_count,
            segment_size,
            segment_count,
            upscale_ratio,
        }
    }

    pub fn export_segment(&self, index: usize) -> Result<BufReader<ChildStderr>, Error> {
        let index_dir = format!("temp\\tmp_frames\\{}", index);
        fs::create_dir(&index_dir).unwrap();

        let output_path = format!("temp\\tmp_frames\\{}\\frame%08d.png", index);
        let start_time = if index == 0 {
            String::from("0")
        } else {
            ((index as u32 * self.segment_size - 1) as f32 / self.frame_rate).to_string()
        };
        let segments_index = if self.segments.len() == 1 { 0 } else { 1 };
        let stderr = Command::new("ffmpeg")
            .args([
                "-v",
                "verbose",
                "-ss",
                &start_time,
                "-i",
                &self.path.to_string(),
                "-qscale:v",
                "1",
                "-qmin",
                "1",
                "-qmax",
                "1",
                "-vsync",
                "0",
                "-vframes",
                &self.segments[segments_index].size.to_string(),
                &output_path,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?
            .stderr
            .ok_or_else(|| Error::new(ErrorKind::Other, "Could not capture standard output."))?;

        Ok(BufReader::new(stderr))
    }

    pub fn upscale_segment(&self, index: usize) -> Result<BufReader<ChildStderr>, Error> {
        let input_path = format!("temp\\tmp_frames\\{}", index);
        let output_path = format!("temp\\out_frames\\{}", index);
        fs::create_dir(&output_path).expect("could not create directory");

        let stderr = Command::new("realesrgan-ncnn-vulkan")
            .args([
                "-i",
                &input_path,
                "-o",
                &output_path,
                "-n",
                "realesr-animevideov3-x2",
                "-s",
                &self.upscale_ratio.to_string(),
                "-f",
                "png",
                "-v",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?
            .stderr
            .ok_or_else(|| Error::new(ErrorKind::Other, "Could not capture standard output."))?;

        Ok(BufReader::new(stderr))
    }

    // TODO: args builder for custom commands
    pub fn merge_segment(&self, args: Vec<&str>) -> Result<BufReader<ChildStderr>, Error> {
        let mut stderr = Command::new("ffmpeg");
        for arg in args {
            stderr.arg(arg);
        }
        let stderr = stderr
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?
            .stderr
            .ok_or_else(|| Error::new(ErrorKind::Other, "Could not capture standard output."))?;

        Ok(BufReader::new(stderr))
    }

    pub fn concatenate_segments(&self) {
        let mut f_content = String::from("file 'video_parts\\0.mp4'");
        for segment_index in 1..self.segment_count {
            let video_part_path = format!("video_parts\\{}.mp4", segment_index);
            f_content = format!("{}\nfile '{}'", f_content, video_part_path);
        }
        fs::write("temp\\parts.txt", f_content).unwrap();

        Command::new("ffmpeg")
            .args([
                "-f",
                "concat",
                "-safe",
                "0",
                "-i",
                "temp\\parts.txt",
                "-i",
                &self.path,
                "-map",
                "0:v",
                "-map",
                "1:a?",
                "-map",
                "1:s?",
                "-map_chapters",
                "1",
                "-c",
                "copy",
                &self.output_path,
            ])
            .output()
            .unwrap();
        fs::remove_file("temp\\parts.txt").unwrap();
    }
}

#[derive(Parser, Serialize, Deserialize, Debug)]
#[clap(name = "Real-ESRGAN Video Enhance",
author = "ONdraid <ondraid.png@gmail.com>",
about = "Real-ESRGAN video upscaler with resumability",
long_about = None)]
pub struct Args {
    /// input video path (mp4/mkv/...) or folder path (\\... or /... or C:\...)
    #[clap(short = 'i', long, value_parser = input_validation)]
    pub inputpath: String,

    // maximum resolution (480 by default)
    #[clap(short = 'r', long, value_parser = max_resolution_validation, default_value = "480")]
    pub resolution: Option<String>,

    // output video extension format (mp4 by default)
    #[clap(short = 'f', long, value_parser = format_validation, default_value = "mp4")]
    pub format: String,

    /// upscale ratio (2, 3, 4)
    #[clap(short = 's', long, value_parser = clap::value_parser!(u8).range(2..5), default_value_t = 2)]
    pub scale: u8,

    /// segment size (in frames)
    #[clap(short = 'P', long = "parts", value_parser, default_value_t = 1000)]
    pub segmentsize: u32,

    /// video constant rate factor (crf: 51-0)
    #[clap(short = 'c', long = "crf", value_parser = clap::value_parser!(u8).range(0..52), default_value_t = 15)]
    pub crf: u8,

    /// video encoding preset
    #[clap(short = 'p', long, value_parser = preset_validation, default_value = "slow")]
    pub preset: String,

    /// codec encoding parameters (libsvt_hevc, libsvtav1, libx265)
    #[clap(
        short = 'e',
        long = "encoder",
        value_parser = codec_validation,
        default_value = "libx265"
    )]
    pub codec: String,

    /// x265 encoding parameters
    #[clap(
        short = 'x',
        long,
        value_parser,
        default_value = "psy-rd=2:aq-strength=1:deblock=0,0:bframes=8"
    )]
    pub x265params: String,

    // (Optional) output video path (file.mp4/mkv/...)
    #[clap(short = 'o', long, value_parser = output_validation)]
    pub outputpath: Option<String>,
}

fn input_validation(s: &str) -> Result<String, String> {
    let p = Path::new(s);

    // if the path in p contains a double quote, remove it and everything after it
    if p.to_str().unwrap().contains("\"") {
        let mut s = p.to_str().unwrap().to_string();
        s.truncate(s.find("\"").unwrap());
        return Ok(s);
    }

    if p.is_dir() {
        return Ok(String::from_str(s).unwrap());
    }

    if !p.exists() {
        return Err(String::from_str("input path not found").unwrap());
    }

    match p.extension().unwrap().to_str().unwrap() {
        "mp4" | "mkv" | "avi" => Ok(s.to_string()),
        _ => Err(String::from_str("valid input formats: mp4/mkv/avi").unwrap()),
    }
}

pub fn output_validation(s: &str) -> Result<String, String> {
    let p = Path::new(s);

    if p.exists() {
        println!("{} already exists!", &s);
        exit(1);
    } else {
        match p.extension().unwrap().to_str().unwrap() {
            "mp4" | "mkv" | "avi" => Ok(s.to_string()),
            _ => Err(String::from_str("valid input formats: mp4/mkv/avi").unwrap()),
        }
    }
}

pub fn output_validation_dir(s: &str) -> Result<String, String> {
    let p = Path::new(s);

    if p.exists() {
        return Ok("already exists".to_string());
    } else {
        match p.extension().unwrap().to_str().unwrap() {
            "mp4" | "mkv" | "avi" => Ok(s.to_string()),
            _ => Err(String::from_str("valid input formats: mp4/mkv/avi").unwrap()),
        }
    }
}

fn format_validation(s: &str) -> Result<String, String> {
    match s {
        "mp4" | "mkv" | "avi" => Ok(s.to_string()),
        _ => Err(String::from_str("valid output formats: mp4/mkv/avi").unwrap()),
    }
}

fn max_resolution_validation(s: &str) -> Result<String, String> {
    let validate = s.parse::<f64>().is_ok();
    match validate {
        true => Ok(s.to_string()),
        false => Err(String::from_str("valid resolution is numeric!").unwrap()),
    }
}

fn preset_validation(s: &str) -> Result<String, String> {
    match s {
        "ultrafast" | "superfast" | "veryfast" | "faster" | "fast" | "medium" | "slow"
        | "slower" | "veryslow" => Ok(s.to_string()),
        _ => Err(String::from_str(
            "valid: ultrafast/superfast/veryfast/faster/fast/medium/slow/slower/veryslow",
        )
        .unwrap()),
    }
}

fn codec_validation(s: &str) -> Result<String, String> {
    match s {
        "libx265" | "libsvt_hevc" | "libsvtav1" => Ok(s.to_string()),
        _ => Err(String::from_str("valid: libx265/libsvt_hevc/libsvtav1").unwrap()),
    }
}

pub fn get_last_segment_size(frame_count: u32, segment_size: u32) -> u32 {
    let last_segment_size = (frame_count % segment_size) as u32;
    if last_segment_size == 0 {
        segment_size
    } else {
        last_segment_size - 1
    }
}

pub fn rebuild_temp(keep_args: bool) {
    let _ = fs::create_dir("temp");
    if !keep_args {
        println!("removing temp");
        fs::remove_dir_all("temp").expect("could not remove temp. try deleting manually");

        for dir in ["temp\\tmp_frames", "temp\\out_frames", "temp\\video_parts"] {
            println!("creating {}", dir);
            fs::create_dir_all(dir).unwrap();
        }
    } else {
        for dir in ["temp\\tmp_frames", "temp\\out_frames"] {
            println!("removing {}", dir);
            fs::remove_dir_all(dir)
                .unwrap_or_else(|_| panic!("could not remove {:?}. try deleting manually", dir));
            println!("creating {}", dir);
            fs::create_dir_all(dir).unwrap();
        }
        println!("removing parts.txt");
        let _ = fs::remove_file("temp\\parts.txt");
    }
}

pub fn add_to_db(
    files: Vec<String>,
    res: String,
    bar: ProgressBar,
) -> Result<(Vec<AtomicI32>, Arc<Mutex<Vec<std::string::String>>>)> {
    let count: AtomicI32 = AtomicI32::new(0);
    let db_count;
    let db_count_added: AtomicI32 = AtomicI32::new(0);
    let db_count_skipped: AtomicI32 = AtomicI32::new(0);
    let files_to_process: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let conn = Connection::open("reve.db")?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS video_info (
                    id INTEGER PRIMARY KEY,
                    filename TEXT NOT NULL,
                    filepath TEXT NOT NULL,
                    width INTEGER NOT NULL,
                    height INTEGER NOT NULL,
                    duration REAL NOT NULL,
                    pixel_format TEXT NOT NULL,
                    display_aspect_ratio TEXT NOT NULL,
                    sample_aspect_ratio TEXT NOT NULL,
                    format TEXT NOT NULL,
                    size BIGINT NOT NULL,
                    folder_size BIGINT NOT NULL,
                    bitrate BIGINT NOT NULL,
                    codec TEXT NOT NULL,
                    resolution TEXT NOT NULL,
                    status TEXT NOT NULL,
                    hash TEXT NOT NULL
                  )",
        params![],
    )?;

    let filenames_skip = files.clone();
    let mut filenames = files;

    // get all items in db
    let mut stmt = conn.prepare("SELECT * FROM video_info")?;
    let mut rows = stmt
        .query_map(params![], |row| {
            Ok((
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
                row.get(11)?,
                row.get(12)?,
                row.get(13)?,
                row.get(14)?,
                row.get(15)?,
                row.get(16)?,
            ))
        })
        .unwrap();
    let mut db_items: Vec<(
        String,
        String,
        i32,
        i32,
        f64,
        String,
        String,
        String,
        String,
        i64,
        i64,
        i64,
        String,
        String,
        String,
        String,
    )> = Vec::new();
    while let Some(row) = rows.next() {
        let row = row.unwrap();
        db_items.push(row);
    }
    // get all items from filenames that are not in db
    let mut filenames_to_process: Vec<String> = Vec::new();
    for filename in filenames {
        let real_filename = Path::new(&filename).file_name().unwrap().to_str().unwrap();
        let mut found = false;
        for item in &db_items {
            if item.0 == real_filename {
                found = true;
                break;
            }
        }
        if !found {
            filenames_to_process.push(filename);
        }
    }

    // get all the items from filenames that are in db
    let mut filenames_to_skip: Vec<String> = Vec::new();
    for filename in filenames_skip {
        let real_filename = Path::new(&filename).file_name().unwrap().to_str().unwrap();
        let mut found = false;
        for item in &db_items {
            if item.0 == real_filename {
                found = true;
                break;
            }
        }
        if found {
            filenames_to_skip.push(filename);
        }
    }
    db_count = AtomicI32::new(filenames_to_skip.len() as i32);

    // print count for all items in filenames_to_process and return filenames with all items in db removed
    println!("Found {} files not in database", filenames_to_process.len());
    filenames = filenames_to_process.clone();

    bar.set_length(filenames.len() as u64);
    let conn = Arc::new(Mutex::new(Connection::open("reve.db")?));

    filenames.par_iter().for_each(|filename| {
        let real_filename = Path::new(filename).file_name().unwrap().to_str().unwrap();
        let conn = conn.clone();
        let conn = conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT * FROM video_info WHERE filename=?1").unwrap();
        let file_exists: bool = stmt.exists(params![real_filename]).unwrap();
        if !file_exists {
            let output = Command::new("ffprobe")
                .args([
                    "-i",
                    filename,
                    "-v",
                    "error",
                    "-select_streams",
                    "v",
                    "-show_entries",
                    "stream",
                    "-show_format",
                    "-show_data_hash",
                    "sha256",
                    "-show_streams",
                    "-of",
                    "json"
                ])
                .output()
                .expect("failed to execute process");
            let json_value: Value = serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).unwrap();
            let json_str = json_value.to_string();
            if &json_str.len() >= &1 {
                let values: Value = json_value;
                let _width = values["streams"][0]["width"].as_i64().unwrap_or(0);
                let _height = values["streams"][0]["height"].as_i64().unwrap_or(0);
                let filepath = values["format"]["filename"].as_str().unwrap();
                let filename = Path::new(filepath).file_name().unwrap().to_str().unwrap();
                let size = values["format"]["size"].as_str().unwrap_or("0");
                let bitrate = values["format"]["bit_rate"].as_str().unwrap_or("0");
                let duration = values["format"]["duration"].as_str().unwrap_or("0.0");
                let format = values["format"]["format_name"].as_str().unwrap_or("NaN");
                let width = values["streams"][0]["width"].as_i64().unwrap_or(0);
                let height = values["streams"][0]["height"].as_i64().unwrap_or(0);
                let codec = values["streams"][0]["codec_name"].as_str().unwrap_or("NaN");
                let pix_fmt = values["streams"][0]["pix_fmt"].as_str().unwrap_or("NaN");
                let checksum = values["streams"][0]["extradata_hash"].as_str().unwrap_or("NaN");
                let dar = values["streams"][0]["display_aspect_ratio"].as_str().unwrap_or("NaN");
                let sar = values["streams"][0]["sample_aspect_ratio"].as_str().unwrap_or("NaN");

                // for each file in this folder and it's subfodlers, sum the size of the files
                let mut folder_size = 0;
                for entry in WalkDir::new(Path::new(filepath).parent().unwrap()) {
                    let entry = entry.unwrap();
                    let metadata = fs::metadata(entry.path());
                    folder_size += metadata.unwrap().len() as i64;
                }
                //println!("{}", folder_size);

                if height <= res.parse::<i64>().unwrap() {
                    conn.execute(
                        "INSERT INTO video_info (filename, filepath, width, height, duration, pixel_format, display_aspect_ratio, sample_aspect_ratio, format, size, folder_size, bitrate, codec, resolution, status, hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                        params![filename, filepath, width, height, duration, pix_fmt, dar, sar, format, size, folder_size, bitrate, codec, res, "pending", checksum]
                    ).unwrap();
                    count.fetch_add(1, Ordering::SeqCst);
                    db_count_added.fetch_add(1, Ordering::SeqCst);
                } else {
                    //db_count_skipped.fetch_add(1, Ordering::SeqCst);
                    conn.execute(
                        "INSERT INTO video_info (filename, filepath, width, height, duration, pixel_format, display_aspect_ratio, sample_aspect_ratio, format, size, folder_size, bitrate, codec, resolution, status, hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                        params![filename, filepath, width, height, duration, pix_fmt, dar, sar, format, size, folder_size, bitrate, codec, res, "skipped", checksum]
                    ).unwrap();
                    count.fetch_add(1, Ordering::SeqCst);
                    db_count_added.fetch_add(1, Ordering::SeqCst);
                }
            }
        }

        // TODO check if all files in db then return only the ones that need to be processed
        let height = get_ffprobe_output(filename).unwrap();
        let height_value = height["streams"][0]["height"].as_i64().unwrap_or(0);
        if height_value <= res.parse::<i64>().unwrap() {
            files_to_process.lock().unwrap().push(filename.to_string());
        }

        bar.inc(1);
    });

    // return all the counters
    Ok((
        vec![count, db_count, db_count_added, db_count_skipped],
        files_to_process,
    ))
}

pub fn update_db_status(
    conn: &Connection,
    filepath: &str,
    status: &str,
) -> Result<(), rusqlite::Error> {
    let mut stmt = conn.prepare("UPDATE video_info SET status=?1 WHERE filepath=?2")?;
    stmt.execute(params![status, filepath])?;
    Ok(())
}

pub fn get_ffprobe_output(filename: &str) -> Result<Value, String> {
    let output: Output = Command::new("ffprobe")
        .args([
            "-i",
            filename,
            "-v",
            "error",
            "-select_streams",
            "v",
            "-show_entries",
            "stream",
            "-show_format",
            "-show_data_hash",
            "sha256",
            "-show_streams",
            "-of",
            "json",
        ])
        .output()
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        let output_str = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
        let value: Value = from_str(&output_str).map_err(|e| e.to_string())?;
        Ok(value)
    } else {
        Err(String::from_utf8(output.stderr).unwrap_or_else(|e| e.to_string()))
    }
}

#[cfg(target_os = "linux")]
pub fn dev_shm_exists() -> Result<(), std::io::Error> {
    let path = "/dev/shm";
    let b: bool = Path::new(path).is_dir();

    if b == true {
        fs::create_dir_all("/dev/shm/tmp_frames")?;
        fs::create_dir_all("/dev/shm/out_frames")?;
        fs::create_dir_all("/dev/shm/video_parts")?;
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "dev/shm does not exist!",
        ))
    }
}

pub fn copy_streams_no_bin_data(
    video_input_path: &String,
    copy_input_path: &String,
    output_path: &String,
    //ffmpeg_args: &String,
) -> std::process::Output {
    Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-v",
            "error",
            "-y",
            "-i",
            video_input_path,
            "-i",
            copy_input_path,
            "-map",
            "0:v",
            "-map",
            "1",
            "-map",
            "-1:d",
            "-map",
            "-1:v",
            "-c",
            "copy",
            output_path,
        ])
        .output()
        .expect("failed to execute process")
}

pub fn copy_streams(
    video_input_path: &String,
    copy_input_path: &String,
    output_path: &String,
) -> std::process::Output {
    Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-v",
            "error",
            "-y",
            "-i",
            video_input_path,
            "-i",
            copy_input_path,
            "-map",
            "0:v",
            "-map",
            "1",
            "-map",
            "-1:v",
            "-c",
            "copy",
            output_path,
        ])
        .output()
        .expect("failed to execute process")
}

pub fn absolute_path(path: impl AsRef<Path>) -> String {
    let path = path.as_ref();

    let absolute_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .expect("could not get current path")
            .join(path)
    }
    .clean();

    absolute_path.into_os_string().into_string().unwrap()
}

pub fn walk_count(dir: &String) -> usize {
    let mut count = 0;
    for e in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        if e.metadata().unwrap().is_file() {
            let filepath = e.path().display();
            let str_filepath = filepath.to_string();
            //println!("{}", filepath);
            let mime = find_mimetype(&str_filepath);
            if mime.to_string() == "VIDEO" {
                count = count + 1;
                //println!("{}", e.path().display());
            }
        }
    }
    println!("Found {} valid video files in folder!", count);
    return count;
}

pub fn walk_files(dir: &String) -> Vec<String> {
    let mut arr = vec![];
    let mut index = 0;

    for e in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        if e.metadata().unwrap().is_file() {
            let filepath = e.path().display();
            let str_filepath = filepath.to_string();
            //println!("{}", filepath);
            let mime = find_mimetype(&str_filepath);
            if mime.to_string() == "VIDEO" {
                //println!("{}", e.path().display());
                arr.insert(index, e.path().display().to_string());
                index = index + 1;
            }
        }
    }
    return arr;
}

pub fn find_mimetype(filename: &String) -> String {
    let parts: Vec<&str> = filename.split('.').collect();

    let res = match parts.last() {
        Some(v) => match *v {
            "mkv" => "VIDEO",
            "avi" => "VIDEO",
            "mp4" => "VIDEO",
            "divx" => "VIDEO",
            "flv" => "VIDEO",
            "m4v" => "VIDEO",
            "mov" => "VIDEO",
            "ogv" => "VIDEO",
            "ts" => "VIDEO",
            "webm" => "VIDEO",
            "wmv" => "VIDEO",
            &_ => "OTHER",
        },
        None => "OTHER",
    };
    return res.to_string();
}

pub fn check_ffprobe_output_i8(data: &str, res: &str) -> Result<i8, Error> {
    let to_process;
    let values: Value = serde_json::from_str(data)?;
    let height = &values["streams"][0]["height"];
    let u8_height = height.as_i64().unwrap();
    let u8_res: i64 = res.parse().unwrap();

    if u8_res >= u8_height {
        to_process = 1;
    } else {
        to_process = 0;
    }

    return Ok(to_process);
}

pub fn get_frame_count(input_path: &String) -> u32 {
    let output = Command::new("ffprobe")
        .arg("-i")
        .arg(input_path)
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v")
        .arg("-show_entries")
        .arg("stream=nb_frames")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=1")
        .output()
        .expect("failed to execute process");
    let r = String::from_utf8(output.stdout)
        .unwrap()
        .trim()
        .parse::<u32>();
    match r {
        Err(_e) => 0,
        _ => r.unwrap(),
    }
}

pub fn get_frame_count_tag(input_path: &String) -> u32 {
    let output = Command::new("ffprobe")
        .arg("-i")
        .arg(input_path)
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v")
        .arg("-show_entries")
        .arg("stream_tags=NUMBER_OF_FRAMES-eng")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=1")
        .output()
        .expect("failed to execute process");
    let r = String::from_utf8(output.stdout)
        .unwrap()
        .trim()
        .parse::<u32>();
    match r {
        Err(_e) => 0,
        _ => r.unwrap(),
    }
}

pub fn get_frame_count_duration(input_path: &String) -> u32 {
    let output = Command::new("ffprobe")
        .arg("-i")
        .arg(input_path)
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v")
        .arg("-show_entries")
        .arg("format=duration")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=1")
        .output()
        .expect("failed to execute process");
    let r = String::from_utf8(output.stdout)
        .unwrap()
        .trim()
        .parse::<f32>();
    match r {
        Err(_e) => 0,
        _ => (r.unwrap() * 25.0) as u32,
    }
}

pub fn get_display_aspect_ratio(input_path: &String) -> String {
    let output = Command::new("ffprobe")
        .arg("-i")
        .arg(input_path)
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v")
        .arg("-show_entries")
        .arg("stream=display_aspect_ratio")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=1")
        .output()
        .expect("failed to execute process");
    let r = String::from_utf8(output.stdout)
        .unwrap()
        .trim()
        .parse::<String>();
    match r {
        Err(_e) => "0".to_owned(),
        _ => r.unwrap(),
    }
}

pub fn get_frame_rate(input_path: &String) -> String {
    let output = Command::new("ffprobe")
        .arg("-i")
        .arg(input_path)
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v")
        .arg("-show_entries")
        .arg("stream=avg_frame_rate")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=1")
        .output()
        .expect("failed to execute process");

    let temp_output = output.clone();
    let raw_framerate = String::from_utf8(temp_output.stdout)
        .unwrap()
        .trim()
        .to_string();
    let split_framerate = raw_framerate.split("/");
    let vec_framerate: Vec<&str> = split_framerate.collect();
    let frames: f32 = vec_framerate[0].parse().unwrap();
    let seconds: f32 = vec_framerate[1].parse().unwrap();
    return (frames / seconds).to_string();
}

pub fn get_bin_data(input_path: &String) -> String {
    let output = Command::new("ffprobe")
        .arg("-i")
        .arg(input_path)
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("d")
        .arg("-show_entries")
        .arg("stream=index")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=1")
        .output()
        .expect("failed to execute process");

    let temp_output = output.clone();
    let bin_data = String::from_utf8(temp_output.stdout)
        .unwrap()
        .trim()
        .to_string();
    return bin_data;
}

pub fn export_frames(
    input_path: &String,
    output_path: &String,
    start_time: &String,
    frame_number: &u32,
    progress_bar: ProgressBar,
) -> Result<(), Error> {
    let stderr = Command::new("ffmpeg")
        .args([
            "-v",
            "verbose",
            "-ss",
            start_time,
            "-i",
            input_path,
            "-qscale:v",
            "1",
            "-qmin",
            "1",
            "-qmax",
            "1",
            "-vsync",
            "0",
            "-vframes",
            &frame_number.to_string(),
            output_path,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?
        .stderr
        .ok_or_else(|| Error::new(ErrorKind::Other, "Could not capture standard output."))?;

    let reader = BufReader::new(stderr);
    let mut count: i32 = -1;

    reader
        .lines()
        .filter_map(|line| line.ok())
        .filter(|line| line.contains("AVIOContext"))
        .for_each(|_| {
            count += 1;
            progress_bar.set_position(count as u64);
        });

    Ok(())
}

pub fn upscale_frames(
    input_path: &String,
    output_path: &String,
    scale: &String,
    progress_bar: ProgressBar,
    total_progress_bar: ProgressBar,
    mut frame_position: u64,
) -> Result<u64, Error> {
    #[cfg(target_os = "linux")]
    let stderr = Command::new("./realesrgan-ncnn-vulkan")
        .args([
            "-i",
            input_path,
            "-o",
            output_path,
            "-n",
            "realesr-animevideov3-x2",
            "-s",
            scale,
            "-f",
            "png",
            "-v",
        ])
        .stderr(Stdio::piped())
        .spawn()?
        .stderr
        .ok_or_else(|| Error::new(ErrorKind::Other, "Could not capture standard output."))?;

    #[cfg(target_os = "windows")]
    let stderr = Command::new("realesrgan-ncnn-vulkan")
        .args([
            "-i",
            input_path,
            "-o",
            output_path,
            "-n",
            "realesr-animevideov3-x2",
            "-s",
            scale,
            "-f",
            "png",
            "-v",
        ])
        .stderr(Stdio::piped())
        .spawn()?
        .stderr
        .ok_or_else(|| Error::new(ErrorKind::Other, "Could not capture standard output."))?;

    let reader = BufReader::new(stderr);
    let mut count = 0;

    total_progress_bar.set_position(frame_position);
    //println!("{}", frame_position);

    reader
        .lines()
        .filter_map(|line| line.ok())
        .filter(|line| line.contains("done"))
        .for_each(|_| {
            count += 1;
            frame_position += 1;
            progress_bar.set_position(count);
            total_progress_bar.set_position(frame_position);
        });

    Ok(u64::from(total_progress_bar.position()))
}

// 2022-05-23 17:47 27cffd1
// https://github.com/AnimMouse/ffmpeg-autobuild/releases/download/m-2022-05-23-17-47/ffmpeg-27cffd1-ff31946-win64-nonfree.7z
pub fn merge_frames(
    input_path: &String,
    output_path: &String,
    codec: &String,
    frame_rate: &String,
    crf: &String,
    preset: &String,
    x265_params: &String,
    progress_bar: ProgressBar,
) -> Result<(), Error> {
    let stderr = Command::new("ffmpeg")
        .args([
            "-v",
            "verbose",
            "-f",
            "image2",
            "-framerate",
            &format!("{}/1", frame_rate),
            "-i",
            input_path,
            "-c:v",
            codec,
            "-pix_fmt",
            "yuv420p10le",
            "-crf",
            crf,
            "-preset",
            preset,
            "-x265-params",
            x265_params,
            output_path,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?
        .stderr
        .ok_or_else(|| Error::new(ErrorKind::Other, "Could not capture standard output."))?;

    let reader = BufReader::new(stderr);
    let mut count = 0;

    reader
        .lines()
        .filter_map(|line| line.ok())
        .filter(|line| line.contains("AVIOContext"))
        .for_each(|_| {
            count += 1;
            progress_bar.set_position(count);
        });
    Ok(())
}

// 2022-03-28 07:12 c2d1597
// https://github.com/AnimMouse/ffmpeg-autobuild/releases/download/m-2022-03-28-07-12/ffmpeg-c2d1597-651202b-win64-nonfree.7z
pub fn merge_frames_svt_hevc(
    input_path: &String,
    output_path: &String,
    codec: &String,
    frame_rate: &String,
    crf: &String,
    progress_bar: ProgressBar,
) -> Result<(), Error> {
    let stderr = Command::new("ffmpeg")
        .args([
            "-v",
            "verbose",
            "-f",
            "image2",
            "-framerate",
            &format!("{}/1", frame_rate),
            "-i",
            input_path,
            "-c:v",
            codec,
            "-rc",
            "0",
            "-qp",
            crf,
            "-tune",
            "0",
            "-pix_fmt",
            "yuv420p10le",
            "-crf",
            crf,
            output_path,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?
        .stderr
        .ok_or_else(|| Error::new(ErrorKind::Other, "Could not capture standard output."))?;

    let reader = BufReader::new(stderr);
    let mut count = 0;

    reader
        .lines()
        .filter_map(|line| line.ok())
        .filter(|line| line.contains("AVIOContext"))
        .for_each(|_| {
            count += 1;
            progress_bar.set_position(count);
        });

    Ok(())
}

pub fn merge_frames_svt_av1(
    input_path: &String,
    output_path: &String,
    codec: &String,
    frame_rate: &String,
    crf: &String,
    progress_bar: ProgressBar,
) -> Result<(), Error> {
    let stderr = Command::new("ffmpeg")
        .args([
            "-v",
            "verbose",
            "-f",
            "image2",
            "-framerate",
            &format!("{}/1", frame_rate),
            "-i",
            input_path,
            "-c:v",
            codec,
            "-pix_fmt",
            "yuv420p10le",
            "-crf",
            crf,
            output_path,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?
        .stderr
        .ok_or_else(|| Error::new(ErrorKind::Other, "Could not capture standard output."))?;

    let reader = BufReader::new(stderr);
    let mut count = 0;

    reader
        .lines()
        .filter_map(|line| line.ok())
        .filter(|line| line.contains("AVIOContext"))
        .for_each(|_| {
            count += 1;
            progress_bar.set_position(count);
        });

    Ok(())
}

pub fn merge_video_parts_dar(
    input_path: &String,
    output_path: &String,
    dar: &String,
) -> std::process::Output {
    Command::new("ffmpeg")
        .args([
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
            input_path,
            "-aspect",
            dar,
            "-c",
            "copy",
            output_path,
        ])
        .output()
        .expect("failed to execute process")
}

pub fn merge_video_parts(input_path: &String, output_path: &String) -> std::process::Output {
    Command::new("ffmpeg")
        .args([
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
            input_path,
            "-c",
            "copy",
            output_path,
        ])
        .output()
        .expect("failed to execute process")
}
