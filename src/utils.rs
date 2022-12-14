use colored::Colorize;
use indicatif::{ProgressBar};
use path_clean::PathClean;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Error, ErrorKind};
use std::path::{Path};
use std::process::{Command, Stdio};
use walkdir::WalkDir;
use serde_json::{Value};

pub fn check_bins() {
    #[cfg(target_os = "windows")]
    let realesrgan = std::path::Path::new("realesrgan-ncnn-vulkan.exe").exists();
    #[cfg(target_os = "linux")]
    let realesrgan = std::path::Path::new("realesrgan-ncnn-vulkan").exists();
    #[cfg(target_os = "windows")]
    let ffmpeg = std::path::Path::new("ffmpeg.exe").exists();
    #[cfg(target_os = "linux")]
    let ffmpeg = std::path::Path::new("ffmpeg").exists();
    #[cfg(target_os = "windows")]
    let ffprobe = std::path::Path::new("ffprobe.exe").exists();
    #[cfg(target_os = "linux")]
    let ffprobe = std::path::Path::new("ffprobe").exists();
    #[cfg(target_os = "windows")]    
    let model = std::path::Path::new("models\\realesr-animevideov3-x2.bin").exists();
    #[cfg(target_os = "linux")]
    let model = std::path::Path::new("models/realesr-animevideov3-x2.bin").exists();

    if realesrgan == true {
        println!("{}", String::from("realesrgan-ncnn-vulkan exists!").green().bold());
    } else {
        println!("{}", String::from("realesrgan-ncnn-vulkan does not exist!").red().bold());
        std::process::exit(1);
    }
    if ffmpeg == true {
        println!("{}", String::from("ffmpeg exists!").green().bold());
    } else {
        let ffmpeg_path = Command::new("ffmpeg");
        assert_eq!(ffmpeg_path.get_program(), "ffmpeg");
    }
    if ffprobe == true {
        println!("{}", String::from("ffprobe exists!").green().bold());
    } else {
        let ffmpeg_path = Command::new("ffprobe");
        assert_eq!(ffmpeg_path.get_program(), "ffprobe");
    }
    if model == true {
        println!("{}", String::from("models\\realesr-animevideov3-x2.bin exists!").green().bold());
    } else {
        println!("{}", String::from("models\\realesr-animevideov3-x2.bin does not exist!").red().bold());
        std::process::exit(1);
    }
}

pub fn check_ffmpeg() -> String {
    let output = Command::new("ffmpeg").stdout(Stdio::piped()).output().unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    struct ValidCodecs {
        libsvt_hevc: String,
        libsvtav1: String,
        libx265: String,
    }

    impl Default for ValidCodecs {
        fn default() -> ValidCodecs {
            ValidCodecs {
                libsvt_hevc: 0.to_string(),
                libsvtav1: 0.to_string(),
                libx265: 0.to_string(),
            }
        }
    }

    let mut valid_codecs = ValidCodecs {
        libsvt_hevc: 0.to_string(),
        libsvtav1: 0.to_string(),
        libx265: 0.to_string(),
    };

    if stderr.contains("libsvthevc") {
        println!("{}",format!("libsvt_hevc supported!").green());
        valid_codecs.libsvt_hevc = "libsvt_hevc".to_string();
    } else {
        println!("{}",format!("libsvt_hevc not supported!").red());
        valid_codecs.libsvt_hevc = "".to_string();
    }
    if stderr.contains("libsvtav1") {
        println!("{}",format!("libsvtav1 supported!").green());
        valid_codecs.libsvtav1 = "libsvtav1".to_string();
    } else {
        println!("{}",format!("libsvtav1 not supported!").red());
        valid_codecs.libsvtav1 = "".to_string();
    }
    if stderr.contains("libx265") {
        println!("{}",format!("libx265 supported!").green());
        valid_codecs.libx265 = "libx265".to_string();
    } else {
        println!("{}",format!("libx265 not supported!").red());
        valid_codecs.libx265 = "".to_string();
    }

    let codec_support = String::from(format!("{} {} {}", valid_codecs.libsvt_hevc.to_string(), valid_codecs.libsvtav1.to_string(), valid_codecs.libx265.to_string()));
    return codec_support;

}

// fn create_dirs() -> std::io::Result<()> {
pub fn create_dirs() -> Result<(), std::io::Error> {
    fs::create_dir_all("temp\\tmp_frames\\")?;
    fs::create_dir_all("temp\\video_parts\\")?;
    fs::create_dir_all("temp\\out_frames\\")?;
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn dev_shm_exists() -> Result<(), std::io::Error> {
    let path = "/dev/shm";
    let b: bool = Path::new(path).is_dir();

    if b == true {
        fs::create_dir_all("/dev/shm/tmp_frames")?;
        fs::create_dir_all("/dev/shm/out_frames")?;
        fs::create_dir_all("/dev/shm/video_parts")?;
    }
    Ok(())
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
            output_path
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
            output_path
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

pub fn clear_dirs(dirs: &[&str]) {
    for dir in dirs {
        match fs::remove_dir_all(dir) {
            Ok(_) => (),
            Err(_) => fs::remove_dir_all(dir).unwrap(),
        };
        fs::create_dir(dir).unwrap();
    }
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
                count = count+1;
                //println!("{}", e.path().display());
            }
        }
    }
    println!("Found {} valid video files in folder!", count);
    return count;
}

pub fn walk_files(dir: &String) -> Vec<String>{
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

pub fn find_mimetype(filename :&String) -> String{

    let parts : Vec<&str> = filename.split('.').collect();

    let res = match parts.last() {
            Some(v) =>
                match *v {
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

pub fn check_ffprobe_output(data: &str, res: &str, file: &str) -> Result<Vec<String>, Error> {
    let mut arr: Vec<std::string::String> = vec![];
    let index = 0;
    let v: Value = serde_json::from_str(data)?;
    let height = &v["streams"][0]["height"];
    let u8_height = height.as_i64().unwrap();
    let u8_res: i64 = res.parse().unwrap();

    if u8_res >= u8_height {
        arr.insert(index, file.to_string());
    } else {
        arr.insert(index, "nope".to_string());
    }

    return Ok(arr);
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
    let raw_framerate = String::from_utf8(temp_output.stdout).unwrap().trim().to_string();
    let split_framerate = raw_framerate.split("/");
    let vec_framerate: Vec<&str> = split_framerate.collect();
    println!("{}", vec_framerate[0]);
    println!("{}", vec_framerate[1]);
    let frames: f32 = vec_framerate[0].parse().unwrap();
    let seconds: f32 = vec_framerate[1].parse().unwrap();
    //return String::from_utf8(output.stdout).unwrap().trim().to_string();
    return (frames/seconds).to_string();
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
    let bin_data = String::from_utf8(temp_output.stdout).unwrap().trim().to_string();
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
) -> Result<(), Error> {
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

    reader
        .lines()
        .filter_map(|line| line.ok())
        .filter(|line| line.contains("done"))
        .for_each(|_| {
            count += 1;
            progress_bar.set_position(count);
        });

    Ok(())
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