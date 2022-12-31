use clap::Parser;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufReader, Error, ErrorKind};
use std::path::Path;
use std::process::{ChildStderr, Command, Stdio};
use std::str::FromStr;

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
            segment_count,
            upscale_ratio,
        }
    }

    pub fn export_segment(&self, index: usize) -> Result<BufReader<ChildStderr>, Error> {
        let index_dir = format!("temp\\tmp_frames\\{}", index);
        fs::create_dir(&index_dir).expect("could not create directory");

        let output_path = format!("temp\\tmp_frames\\{}\\frame%08d.png", index);
        let start_time = if index == 0 {
            String::from("0")
        } else {
            ((index as u32 * self.segments[index].size - 1) as f32 / self.frame_rate).to_string()
        };
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
                &self.segments[index].size.to_string(),
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
    /// input video path (mp4/mkv)
    #[clap(short = 'i', long, value_parser = input_validation)]
    pub inputpath: String,

    /// output video path (mp4/mkv)
    #[clap(value_parser = output_validation)]
    pub outputpath: String,

    /// upscale ratio (2, 3, 4)
    #[clap(short = 's', long, value_parser = clap::value_parser!(u8).range(2..5))]
    pub scale: u8,

    /// segment size (in frames)
    #[clap(short = 'S', long, value_parser, default_value_t = 1000)]
    pub segmentsize: u32,

    /// video constant rate factor (crf: 51-0)
    #[clap(short = 'c', long, value_parser = clap::value_parser!(u8).range(0..52), default_value_t = 15)]
    pub crf: u8,

    /// video encoding preset
    #[clap(short = 'p', long, value_parser = preset_validation, default_value = "slow")]
    pub preset: String,

    /// x265 encoding parameters
    #[clap(
        short = 'x',
        long,
        value_parser,
        default_value = "psy-rd=2:aq-strength=1:deblock=0,0:bframes=8"
    )]
    pub x265params: String,
}

fn input_validation(s: &str) -> Result<String, String> {
    let p = Path::new(s);
    if !p.exists() {
        return Err(String::from_str("input path not found").unwrap());
    }
    match p.extension().unwrap().to_str().unwrap() {
        "mp4" | "mkv" => Ok(s.to_string()),
        _ => Err(String::from_str("valid input formats: mp4/mkv").unwrap()),
    }
}

fn output_validation(s: &str) -> Result<String, String> {
    let p = Path::new(s);
    if p.exists() {
        return Err(String::from_str("output path already exists").unwrap());
    }
    match p.extension().unwrap().to_str().unwrap() {
        "mp4" | "mkv" => Ok(s.to_string()),
        _ => Err(String::from_str("valid output formats: mp4/mkv").unwrap()),
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

pub fn get_last_segment_size(frame_count: u32, segment_size: u32) -> u32 {
    let last_segment_size = (frame_count % segment_size) as u32;
    if last_segment_size == 0 {
        segment_size
    } else {
        last_segment_size
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
