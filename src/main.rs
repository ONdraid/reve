use clap::Parser;
use clearscreen::clear;
use colored::Colorize;
use dialoguer::Confirm;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use path_clean::PathClean;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Error, ErrorKind};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::{thread, time::Duration};
use std::time::Instant;

#[derive(Parser, Serialize, Deserialize, Debug)]
#[clap(name = "Real-ESRGAN Video Enhance",
       author = "ONdraid <ondraid.png@gmail.com>",
       about = "Real-ESRGAN video upscaler with resumability",
       long_about = None)]
struct Args {
    /// input video path (mp4/mkv)
    #[clap(short = 'i', long, value_parser = input_validation)]
    inputpath: String,

    /// output video path (mp4/mkv)
    #[clap(value_parser = output_validation)]
    outputpath: String,

    /// upscale ratio (2, 3, 4)
    #[clap(short = 's', long, value_parser = clap::value_parser!(u8).range(2..5))]
    scale: u8,

    /// segment size (in frames)
    #[clap(short = 'P', long = "parts", value_parser, default_value_t = 1000)]
    segmentsize: u32,

    /// video constant rate factor (crf: 51-0)
    #[clap(short = 'c', long = "crf", value_parser = clap::value_parser!(u8).range(0..52), default_value_t = 15)]
    crf: u8,

    /// video encoding preset
    #[clap(short = 'p', long, value_parser = preset_validation, default_value = "slow")]
    preset: String,

    /// codec encoding parameters (libsvt_hevc, libsvtav1, libx265)
    #[clap(
        short = 'e',
        long = "encoder",
        value_parser = codec_validation,
        default_value = "libx265"
    )]
    codec: String,

    /// x265 encoding parameters
    #[clap(
        short = 'x',
        long,
        value_parser,
        default_value = "psy-rd=2:aq-strength=1:deblock=0,0:bframes=8"
    )]
    x265params: String,
}

struct Segment {
    index: u32,
    size: u32,
}

fn input_validation(s: &str) -> Result<String, String> {
    let p = Path::new(s);
    if !p.exists() {
        return Err(String::from_str("input path not found").unwrap());
    }
    match p.extension().unwrap().to_str().unwrap() {
        "mp4" | "mkv" | "avi" => Ok(s.to_string()),
        _ => Err(String::from_str("valid input formats: mp4/mkv/avi").unwrap()),
    }
}

fn output_validation(s: &str) -> Result<String, String> {
    let p = Path::new(s);
    if p.exists() {
        return Err(String::from_str("output path already exists").unwrap());
    }
    match p.extension().unwrap().to_str().unwrap() {
        "mp4" | "mkv" | "avi" => Ok(s.to_string()),
        _ => Err(String::from_str("valid output formats: mp4/mkv/avi").unwrap()),
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
        _ => Err(String::from_str(
            "valid: libx265/libsvt_hevc/libsvtav1",
        )
        .unwrap()),
    }
}

fn extract_realesrgan() {
    sevenz_rust::decompress_file("assets/realesrgan-ncnn-vulkan.7z", ".").expect("complete");
}

fn extract_ffmpeg() {
    sevenz_rust::decompress_file("assets/ffmpeg.7z", ".").expect("complete");
}

fn extract_mediainfo() {
    sevenz_rust::decompress_file("assets/mediainfo.7z", ".").expect("complete");
}

fn extract_models() {
    sevenz_rust::decompress_file("assets/models.7z", ".").expect("complete");
}

fn check_bins() {
    let realesrgan = std::path::Path::new("realesrgan-ncnn-vulkan.exe").exists();
    let ffmpeg = std::path::Path::new("ffmpeg.exe").exists();
    let mediainfo = std::path::Path::new("mediainfo.exe").exists();
    let model = std::path::Path::new("models\\realesr-animevideov3-x2.bin").exists();

    if realesrgan == true {
        println!("{}", String::from("realesrgan-ncnn-vulkan.exe exists!").green().bold());
    } else {
        println!("{}", String::from("realesrgan-ncnn-vulkan.exe does not exist!").red().bold());
        extract_realesrgan();
        println!("{}", String::from("Extracted to bin folder.").green().bold());
        std::process::exit(1);
    }
    if ffmpeg == true {
        println!("{}", String::from("ffmpeg.exe exists!").green().bold());
    } else {
        println!("{}", String::from("ffmpeg.exe does not exist!").red().bold());
        extract_ffmpeg();
        println!("{}", String::from("Extracted to bin folder.").green().bold());
        std::process::exit(1);
    }
    if mediainfo == true {
        println!("{}", String::from("mediainfo.exe exists!").green().bold());
    } else {
        println!("{}", String::from("mediainfo.exe does not exist!").red().bold());
        extract_mediainfo();
        println!("{}", String::from("Extracted to bin folder.").green().bold());
        std::process::exit(1);
    }
    if model == true {
        println!("{}", String::from("models\\realesr-animevideov3-x2.bin exists!").green().bold());
    } else {
        println!("{}", String::from("models\\realesr-animevideov3-x2.bin does not exist!").red().bold());
        extract_models();
        println!("{}", String::from("Extracted to bin folder.").green().bold());
        std::process::exit(1);
    }
}

fn check_ffmpeg() -> String {
    let output = Command::new("ffmpeg.exe").stdout(Stdio::piped()).output().unwrap();
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
fn create_dirs() -> Result<(), std::io::Error> {
    fs::create_dir_all("temp\\tmp_frames\\")?;
    fs::create_dir_all("temp\\video_parts\\")?;
    fs::create_dir_all("temp\\out_frames\\")?;
    Ok(())
}

fn main() {
    let current_exe_path = env::current_exe().unwrap();
    let now = Instant::now();

    // Try to create directories needed
    match create_dirs() {
        Err(e) => println!("{:?}", e),
        _ => ()
    }

    check_bins();

    let input_path;
    let output_path;
    let tmp_frames_path = "temp\\tmp_frames\\";
    let out_frames_path = "temp\\out_frames\\";
    let video_parts_path = "temp\\video_parts\\";
    let temp_video_path = "temp\\temp.mp4";
    let txt_list_path = "temp\\parts.txt";
    let args_path = current_exe_path
        .parent()
        .unwrap()
        .join("temp\\args.temp")
        .into_os_string()
        .into_string()
        .unwrap();

    let mut args;
    args = Args::parse();

    let ffmpeg_support = check_ffmpeg();
    let choosen_codec = &args.codec;
    //println!("{}", choosen_codec);
    //println!("{}", choosen_codec);
    if ffmpeg_support.contains(choosen_codec) {
        println!("Codec {} supported by current ffmpeg binary!", choosen_codec);
        //std::process::exit(1);
    } else {
        println!("Codec {} not supported by current ffmpeg binary! Supported:{}", choosen_codec, ffmpeg_support);
        std::process::exit(1);
    }

    if Path::new(&args_path).exists() {
        clear().expect("failed to clear screen");
        println!("{}", "found existing temporary files.".to_string().red());

        if !Confirm::new()
            .with_prompt("resume upscaling previous video?")
            .default(true)
            .show_default(true)
            .interact()
            .unwrap()
        {
            if !Confirm::new()
                .with_prompt("all progress will be lost. do you want to continue?")
                .default(true)
                .show_default(true)
                .interact()
                .unwrap()
            {
                // Abort remove
                std::process::exit(1);
            }

            // Remove and start new
            args = Args::parse();
            input_path = absolute_path(PathBuf::from_str(&args.inputpath).unwrap());
            args.inputpath = input_path.clone();
            output_path = absolute_path(PathBuf::from_str(&args.outputpath).unwrap());
            args.outputpath = output_path.clone();
            env::set_current_dir(current_exe_path.parent().unwrap()).unwrap();

            clear_dirs(&[tmp_frames_path, out_frames_path, video_parts_path]);
            match fs::remove_file(txt_list_path) {
                Ok(()) => "ok",
                Err(_e) if _e.kind() == ErrorKind::NotFound => "not found",
                Err(_e) => "other",
            };
            match fs::remove_file(temp_video_path) {
                Ok(()) => "ok",
                Err(_e) if _e.kind() == ErrorKind::NotFound => "not found",
                Err(_e) => "other",
            };

            let serialized_args = serde_json::to_string(&args).unwrap();
            fs::remove_file(&args_path).expect("Unable to delete file");
            fs::write(&args_path, serialized_args).expect("Unable to write file");
            clear().expect("failed to clear screen");
            println!(
                "{}",
                "deleted all temporary files, parsing console input"
                    .to_string()
                    .green()
            );
        } else {
            // Resume upscale
            let args_json = fs::read_to_string(&args_path).expect("Unable to read file");
            args = serde_json::from_str(&args_json).unwrap();
            input_path = args.inputpath.clone();
            output_path = args.outputpath.clone();
            env::set_current_dir(current_exe_path.parent().unwrap()).unwrap();

            clear_dirs(&[tmp_frames_path, out_frames_path]);
            clear().expect("failed to clear screen");
            println!("{}", "resuming upscale".to_string().green());
        }
    } else {
        // Start new
        args = Args::parse();

        input_path = absolute_path(PathBuf::from_str(&args.inputpath).unwrap());
        args.inputpath = input_path.clone();
        output_path = absolute_path(PathBuf::from_str(&args.outputpath).unwrap());
        args.outputpath = output_path.clone();
        env::set_current_dir(current_exe_path.parent().unwrap()).unwrap();

        clear_dirs(&[tmp_frames_path, out_frames_path, video_parts_path]);
        let serialized_args = serde_json::to_string(&args).unwrap();
        fs::write(&args_path, serialized_args).expect("Unable to write file");
    }

    // Validation
    {
        let in_extension = Path::new(&input_path).extension().unwrap();
        let out_extension = Path::new(&output_path).extension().unwrap();

        if in_extension == "mkv" && out_extension != "mkv" {
            clear().expect("failed to clear screen");
            println!(
                "{} Invalid value {} for '{}': mkv file can only be exported as mkv file\n\nFor more information try {}",
                "error:".to_string().bright_red(),
                format!("\"{}\"", args.inputpath).yellow(),
                "--outputpath <OUTPUTPATH>".to_string().yellow(),
                "--help".to_string().green()
            );
            std::process::exit(1);
        }
    }

    let total_frame_count = get_frame_count(&input_path);
    let original_frame_rate = get_frame_rate(&input_path);

    // Calculate steps
    let parts_num = (total_frame_count as f32 / args.segmentsize as f32).ceil() as i32;
    let last_part_size = (total_frame_count % args.segmentsize) as u32;
    let last_part_size = if last_part_size == 0 {
        args.segmentsize
    } else {
        last_part_size
    };

    let _codec = args.codec.clone();
    clear().expect("failed to clear screen");
    println!(
        "{}",
        format!(
            "total segments: {}, last segment size: {}, codec: {} (ctrl+c to exit)",
            parts_num, last_part_size, _codec
        )
        .red()
    );

    {
        let mut unprocessed_indexes = Vec::new();
        for i in 0..parts_num {
            let n = format!("{}\\{}.mp4", video_parts_path, i);
            let p = Path::new(&n);
            let frame_number = if i + 1 == parts_num {
                last_part_size
            } else {
                args.segmentsize
            };
            if !p.exists() {
                unprocessed_indexes.push(Segment {
                    index: i as u32,
                    size: frame_number as u32,
                });
            } else {
                let c = get_frame_count(&p.display().to_string());
                if c != frame_number {
                    fs::remove_file(p).expect("could not remove invalid part, maybe in use?");
                    println!("removed invalid segment file [{}] with {} frame size", i, c);
                    unprocessed_indexes.push(Segment {
                        index: i as u32,
                        size: frame_number as u32,
                    });
                }
            }
        }

        let mut export_handle = thread::spawn(move || {});
        let mut merge_handle = thread::spawn(move || {});
        let info_style = "[info][{elapsed_precise}] [{wide_bar:.green/white}] {pos:>7}/{len:7} processed segments       eta: {eta:<7}";
        let expo_style = "[expo][{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos:>7}/{len:7} exporting segment        {per_sec:<12}";
        let upsc_style = "[upsc][{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos:>7}/{len:7} upscaling segment        {per_sec:<12}";
        let merg_style = "[merg][{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos:>7}/{len:7} merging segment          {per_sec:<12}";

        let m = MultiProgress::new();
        let pb = m.add(ProgressBar::new(parts_num as u64));
        pb.set_style(
            ProgressStyle::default_bar()
                .template(info_style)
                .unwrap()
                .progress_chars("#>-"),
        );
        let mut last_pb = pb.clone();

        // Initial export
        if !unprocessed_indexes.is_empty() {
            let index = unprocessed_indexes[0].index;
            let _inpt = input_path.clone();
            let _outpt = format!("temp\\tmp_frames\\{}\\frame%08d.png", index);
            let _start_time = if index == 0 {
                String::from("0")
            } else {
                ((index * args.segmentsize - 1) as f32
                    / original_frame_rate.parse::<f32>().unwrap())
                .to_string()
            };
            let _index_dir = format!("temp\\tmp_frames\\{}", index);
            let _frame_number = unprocessed_indexes[0].size;

            let progress_bar = m.insert_after(&last_pb, ProgressBar::new(_frame_number as u64));
            progress_bar.set_style(
                ProgressStyle::default_bar()
                    .template(expo_style)
                    .unwrap()
                    .progress_chars("#>-"),
            );
            last_pb = progress_bar.clone();

            fs::create_dir(&_index_dir).expect("could not create directory");
            export_frames(
                &input_path,
                &_outpt,
                &_start_time,
                &(_frame_number as u32),
                progress_bar,
            )
            .unwrap();
            m.clear().unwrap();
        }

        for _ in 0..unprocessed_indexes.len() {
            let segment = &unprocessed_indexes[0];
            export_handle.join().unwrap();
            if unprocessed_indexes.len() != 1 {
                let index = unprocessed_indexes[1].index;
                let _inpt = input_path.clone();
                let _outpt = format!("temp\\tmp_frames\\{}\\frame%08d.png", index);
                let _start_time = ((index * args.segmentsize - 1) as f32
                    / original_frame_rate.parse::<f32>().unwrap())
                .to_string();
                let _index_dir = format!("temp\\tmp_frames\\{}", index);
                let _frame_number = unprocessed_indexes[1].size;

                let progress_bar = m.insert_after(&last_pb, ProgressBar::new(_frame_number as u64));
                progress_bar.set_style(
                    ProgressStyle::default_bar()
                        .template(expo_style)
                        .unwrap()
                        .progress_chars("#>-"),
                );
                last_pb = progress_bar.clone();

                export_handle = thread::spawn(move || {
                    fs::create_dir(&_index_dir).expect("could not create directory");
                    export_frames(
                        &_inpt,
                        &_outpt,
                        &_start_time,
                        &(_frame_number as u32),
                        progress_bar,
                    )
                    .unwrap();
                });
            } else {
                export_handle = thread::spawn(move || {});
            }

            let inpt_dir = format!("temp\\tmp_frames\\{}", segment.index);
            let outpt_dir = format!("temp\\out_frames\\{}", segment.index);

            fs::create_dir(&outpt_dir).expect("could not create directory");

            let frame_number = unprocessed_indexes[0].size;

            let progress_bar = m.insert_after(&last_pb, ProgressBar::new(frame_number as u64));
            progress_bar.set_style(
                ProgressStyle::default_bar()
                    .template(upsc_style)
                    .unwrap()
                    .progress_chars("#>-"),
            );
            last_pb = progress_bar.clone();

            upscale_frames(&inpt_dir, &outpt_dir, &args.scale.to_string(), progress_bar)
                .expect("could not upscale frames");

            merge_handle.join().unwrap();

            let _codec = args.codec.clone();
            let _inpt = format!("temp\\out_frames\\{}\\frame%08d.png", segment.index);
            let _outpt = format!("temp\\video_parts\\{}.mp4", segment.index);
            let _frmrt = original_frame_rate.clone();
            let _crf = args.crf.clone().to_string();
            let _preset = args.preset.clone();
            let _x265_params = args.x265params.clone();

            let progress_bar = m.insert_after(&last_pb, ProgressBar::new(frame_number as u64));
            progress_bar.set_style(
                ProgressStyle::default_bar()
                    .template(merg_style)
                    .unwrap()
                    .progress_chars("#>-"),
            );
            last_pb = progress_bar.clone();

            merge_handle = thread::spawn(move || {

                // 2022-03-28 07:12 c2d1597
                // https://github.com/AnimMouse/ffmpeg-autobuild/releases/download/m-2022-03-28-07-12/ffmpeg-c2d1597-651202b-win64-nonfree.7z
                fs::remove_dir_all(&inpt_dir).unwrap();
                if &_codec == "libsvt_hevc" {
                    merge_frames_svt_hevc(
                        &_inpt,
                        &_outpt,
                        &_codec,
                        &_frmrt,
                        &_crf,
                        progress_bar,
                    )
                    .unwrap();
                    fs::remove_dir_all(&outpt_dir).unwrap();
                }
                else if &_codec == "libsvtav1" {
                    merge_frames_svt_av1(
                        &_inpt,
                        &_outpt,
                        &_codec,
                        &_frmrt,
                        &_crf,
                        progress_bar,
                    )
                    .unwrap();
                    fs::remove_dir_all(&outpt_dir).unwrap();
                }
                else if &_codec == "libx265" {
                    merge_frames(
                        &_inpt,
                        &_outpt,
                        &_codec,
                        &_frmrt,
                        &_crf,
                        &_preset,
                        &_x265_params,
                        progress_bar,
                    )
                    .unwrap();
                    fs::remove_dir_all(&outpt_dir).unwrap();
                }
            });

            unprocessed_indexes.remove(0);
            pb.set_position((parts_num - unprocessed_indexes.len() as i32 - 1) as u64);
        }
        merge_handle.join().unwrap();
        m.clear().unwrap();
    }

    // Merge video parts
    let mut f_content = "file 'video_parts\\0.mp4'".to_string();

    for part_number in 1..parts_num {
        let video_part_path = format!("video_parts\\{}.mp4", part_number);
        f_content = format!("{}\nfile '{}'", f_content, video_part_path);
    }

    fs::write(txt_list_path, f_content).expect("Unable to write file");

    println!("merging video segments");
    {
        let mut count = 0;
        let p = Path::new(&temp_video_path);
        loop {
            thread::sleep(Duration::from_secs(1));
            if count == 5 {
                panic!("could not merge segments")
            } else if p.exists() {
                if fs::File::open(p).unwrap().metadata().unwrap().len() == 0 {
                    count += 1;
                } else {
                    break;
                }
            } else {
                merge_video_parts(&txt_list_path.to_string(), &temp_video_path.to_string());
                count += 1;
            }
        }
    }

    println!("copying streams");
    copy_streams(&temp_video_path.to_string(), &input_path, &output_path);

    // Validation
    {
        let p = Path::new(&temp_video_path);
        if p.exists() && fs::File::open(p).unwrap().metadata().unwrap().len() != 0 {
            clear_dirs(&[tmp_frames_path, out_frames_path, video_parts_path]);
            fs::remove_file(txt_list_path).expect("Unable to delete file");
            fs::remove_file(&args_path).expect("Unable to delete file");
            fs::remove_file(temp_video_path).expect("Unable to delete file");
        } else {
            panic!("final file validation error: try running again")
        }
    }

    clear().expect("failed to clear screen");
    let elapsed = now.elapsed();
    let seconds = elapsed.as_secs() % 60;
    let minutes = (elapsed.as_secs() / 60) % 60;
    let hours = (elapsed.as_secs() / 60) / 60;

    // if without quotes
    //let ancestors = Path::new(& _file_path).file_name().unwrap().to_str().unwrap();
    //println!("done {} in {}h:{}m:{}s", ancestors, hours, minutes, seconds);
    let ancestors = Path::new(& args.inputpath).file_name().unwrap();
    println!("done {:?} in {}h:{}m:{}s", ancestors, hours, minutes, seconds);
}

fn get_frame_count(input_path: &String) -> u32 {
    let output = Command::new("mediainfo")
        .arg("--Output=Video;%FrameCount%")
        .arg(input_path)
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

fn get_frame_rate(input_path: &String) -> String {
    let output = Command::new("mediainfo")
        .arg("--Output=Video;%FrameRate%")
        .arg(input_path)
        .output()
        .expect("failed to execute process");
    return String::from_utf8(output.stdout).unwrap().trim().to_string();
}

fn export_frames(
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

fn upscale_frames(
    input_path: &String,
    output_path: &String,
    scale: &String,
    progress_bar: ProgressBar,
) -> Result<(), Error> {
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
fn merge_frames(
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
fn merge_frames_svt_hevc(
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
            "23",
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

fn merge_frames_svt_av1(
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

fn merge_video_parts(input_path: &String, output_path: &String) -> std::process::Output {
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

fn copy_streams(
    video_input_path: &String,
    copy_input_path: &String,
    output_path: &String,
) -> std::process::Output {
    Command::new("ffmpeg")
        .args([
            "-i",
            video_input_path,
            "-vn",
            "-i",
            copy_input_path,
            "-c",
            "copy",
            "-map",
            "0:v",
            "-map",
            "1",
            output_path,
        ])
        .output()
        .expect("failed to execute process")
}

fn absolute_path(path: impl AsRef<Path>) -> String {
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

fn clear_dirs(dirs: &[&str]) {
    for dir in dirs {
        match fs::remove_dir_all(dir) {
            Ok(_) => (),
            Err(_) => fs::remove_dir_all(dir).unwrap(),
        };
        fs::create_dir(dir).unwrap();
    }
}
