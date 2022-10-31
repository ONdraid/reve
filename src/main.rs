use clap::Parser;
use clearscreen::clear;
use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Error, ErrorKind};
use std::path::Path;
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::{thread, time::Duration};
use dialoguer::{Confirm};

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
    #[clap(short = 'P', long, value_parser, default_value_t = 1000)]
    segmentsize: u32,

    /// video constant rate factor (crf: 51-0)
    #[clap(short = 'c', long, value_parser = clap::value_parser!(u8).range(0..52), default_value_t = 15)]
    crf: u8,

    /// video encoding preset
    #[clap(short = 'p', long, value_parser = preset_validation, default_value = "slow")]
    preset: String,

    /// x265 encoding parameters
    #[clap(
        short = 'x',
        long,
        value_parser,
        default_value = "psy-rd=2:aq-strength=1:deblock=0,0:bframes=8"
    )]
    x265params: String,
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

fn main() {
    let current_exe_path = env::current_exe().unwrap();

    let tmp_frames_path = current_exe_path
        .parent()
        .unwrap()
        .join("temp\\tmp_frames\\")
        .into_os_string()
        .into_string()
        .unwrap();
    let out_frames_path = current_exe_path
        .parent()
        .unwrap()
        .join("temp\\out_frames\\")
        .into_os_string()
        .into_string()
        .unwrap();
    let video_parts_path = current_exe_path
        .parent()
        .unwrap()
        .join("temp\\video_parts\\")
        .into_os_string()
        .into_string()
        .unwrap();

    let args_path = current_exe_path
        .parent()
        .unwrap()
        .join("temp\\args.temp")
        .into_os_string()
        .into_string()
        .unwrap();
    let txt_list_path = current_exe_path
        .parent()
        .unwrap()
        .join("temp\\parts.txt")
        .into_os_string()
        .into_string()
        .unwrap();

    let args;
    if Path::new(&args_path).exists() {
        clear().expect("failed to clear screen");
        println!(
            "{}",
            format!("found existing temporary files.").red(),
        );

        if !Confirm::new().with_prompt("resume upscaling previous video?").default(true).show_default(true).interact().unwrap() {
            if !Confirm::new().with_prompt("all progress will be lost. do you want to continue?").default(true).show_default(true).interact().unwrap() {
                std::process::exit(1);
            }
            clear_dirs(&[&tmp_frames_path, &out_frames_path, &video_parts_path]);
            match fs::remove_file(&txt_list_path) {
                Ok(()) => "ok",
                Err(_e) if _e.kind() == ErrorKind::NotFound => "not found",
                Err(_e) => "other",
            };
            let temp_video_path = current_exe_path
                .parent()
                .unwrap()
                .join("temp\\temp.mp4")
                .into_os_string()
                .into_string()
                .unwrap();
            match fs::remove_file(&temp_video_path) {
                Ok(()) => "ok",
                Err(_e) if _e.kind() == ErrorKind::NotFound => "not found",
                Err(_e) => "other",
            };
            fs::remove_file(&args_path).expect("Unable to delete file");
            args = Args::parse();
            let serialized_args = serde_json::to_string(&args).unwrap();
            fs::write(&args_path, serialized_args).expect("Unable to write file");
            clear().expect("failed to clear screen");
            println!(
                "{}",
                format!("deleted all temporary files, parsing console input").green()
            );
        } else {
            let args_json = fs::read_to_string(&args_path).expect("Unable to read file");
            args = serde_json::from_str(&args_json).unwrap();
            clear_dirs(&[&tmp_frames_path, &out_frames_path]);
            clear().expect("failed to clear screen");
            println!("{}", format!("resuming upscale").green());
        }
    } else {
        args = Args::parse();
        clear_dirs(&[&tmp_frames_path, &out_frames_path, &video_parts_path]);
        let serialized_args = serde_json::to_string(&args).unwrap();
        fs::write(&args_path, serialized_args).expect("Unable to write file");
    }

    let ffmpeg_path = current_exe_path
        .parent()
        .unwrap()
        .join("bin\\ffmpeg")
        .into_os_string()
        .into_string()
        .unwrap();
    let mediainfo_path = current_exe_path
        .parent()
        .unwrap()
        .join("bin\\mediainfo")
        .into_os_string()
        .into_string()
        .unwrap();
    let realesrgan_path = current_exe_path
        .parent()
        .unwrap()
        .join("bin\\realesrgan-ncnn-vulkan")
        .into_os_string()
        .into_string()
        .unwrap();

    // Validation
    {
        let in_extension = Path::new(&args.inputpath).extension().unwrap();
        let out_extension = Path::new(&args.outputpath).extension().unwrap();

        if in_extension == "mkv" && out_extension != "mkv" {
            clear().expect("failed to clear screen");
            println!(
                "{}{}{}{}{}{}\n\n{}{}",
                format!("error").bright_red(),
                format!(" Invalid value "),
                format!("\"{}\"", args.inputpath).yellow(),
                format!(" for '"),
                format!("--outputpath <OUTPUTPATH>").yellow(),
                format!("': mkv file can only be exported as mkv file"),
                format!("For more information try "),
                format!("--help").green()
            );
            std::process::exit(1);
        }
    }

    let total_frame_count = get_frame_count(&mediainfo_path, &args.inputpath);
    let original_frame_rate = get_frame_rate(&mediainfo_path, &args.inputpath);

    // Calculate steps
    let parts_num = (total_frame_count as f32 / args.segmentsize as f32).ceil() as i32;
    let last_part_size = total_frame_count % args.segmentsize as i32;
    clear().expect("failed to clear screen");
    println!(
        "{}",
        format!(
            "total segments: {}, last segment size: {} (ctrl+c to exit)",
            parts_num, last_part_size
        )
        .red()
    );

    {
        let mut unprocessed_indexes = Vec::new();
        for i in 0..parts_num {
            let n = format!("{}\\{}.mp4", video_parts_path, i);
            let p = Path::new(&n);
            if !p.exists() {
                unprocessed_indexes.push(i);
            } else {
                let c = get_frame_count(&mediainfo_path, &p.display().to_string());
                if c != args.segmentsize as i32 {
                    fs::remove_file(p).expect("could not remove invalid part, maybe in use?");
                    println!("removed invalid segment file [{}] with {} frame size", i, c);
                    unprocessed_indexes.push(i);
                }
            }
        }

        let mut join_handlers = Vec::new();
        let mut merge_handle = thread::spawn(move || {});
        let m = MultiProgress::new();
        let pb = m.add(ProgressBar::new(parts_num as u64));
        pb.set_style(ProgressStyle::default_bar()
          .template("[info][{elapsed_precise}] [{wide_bar:.green/white}] {pos:>7}/{len:7} processed segments       eta: {eta:<7}")
          .unwrap().progress_chars("#>-"));
        let mut last_pb = pb.clone();

        while !unprocessed_indexes.is_empty() {
            let part_index = unprocessed_indexes[0];
            while join_handlers.len() != 2 && join_handlers.len() < unprocessed_indexes.len() {
                let index = unprocessed_indexes[join_handlers.len()];
                let _ffmpeg = ffmpeg_path.clone();
                let _inpt = args.inputpath.clone();
                let _outpt = current_exe_path
                    .parent()
                    .unwrap()
                    .join(format!("temp\\tmp_frames\\{}\\frame%08d.png", index))
                    .into_os_string()
                    .into_string()
                    .unwrap();
                let _start_time = ((index * args.segmentsize as i32 - 1) as f32
                    / original_frame_rate.parse::<f32>().unwrap())
                .to_string();
                let _index_dir = current_exe_path
                    .parent()
                    .unwrap()
                    .join(format!("temp\\tmp_frames\\{}", index))
                    .into_os_string()
                    .into_string()
                    .unwrap();
                let _frame_number = if index + 1 == parts_num && last_part_size != 0 {
                    last_part_size
                } else {
                    args.segmentsize as i32
                };

                let progress_bar = m.insert_after(&last_pb, ProgressBar::new(_frame_number as u64));
                progress_bar.set_style(ProgressStyle::default_bar()
                            .template("[expo][{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos:>7}/{len:7} exporting segment        {per_sec:<12}")
                            .unwrap().progress_chars("#>-"));
                last_pb = progress_bar.clone();

                let thread_join_handle = thread::spawn(move || {
                    fs::create_dir(&_index_dir).expect("could not create directory");
                    export_frames(
                        &_ffmpeg,
                        &_inpt,
                        &_outpt,
                        &_start_time,
                        &(_frame_number as u32),
                        progress_bar,
                    )
                    .unwrap();
                });
                join_handlers.push(thread_join_handle);
            }

            let inpt_dir = current_exe_path
                .parent()
                .unwrap()
                .join(format!("temp\\tmp_frames\\{}", part_index))
                .into_os_string()
                .into_string()
                .unwrap();
            let outpt_dir = current_exe_path
                .parent()
                .unwrap()
                .join(format!("temp\\out_frames\\{}", part_index))
                .into_os_string()
                .into_string()
                .unwrap();

            join_handlers
                .remove(0)
                .join()
                .expect("could not handle thread");

            fs::create_dir(&outpt_dir).expect("could not create directory");

            let frame_number = if part_index + 1 == parts_num && last_part_size != 0 {
                last_part_size
            } else {
                args.segmentsize as i32
            };

            let progress_bar = m.insert_after(&last_pb, ProgressBar::new(frame_number as u64));
            progress_bar.set_style(ProgressStyle::default_bar()
                        .template("[upsc][{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos:>7}/{len:7} upscaling segment        {per_sec:<12}")
                        .unwrap().progress_chars("#>-"));
            last_pb = progress_bar.clone();

            upscale_frames(
                &realesrgan_path,
                &inpt_dir,
                &outpt_dir,
                &args.scale.to_string(),
                progress_bar,
            )
            .expect("could not upscale frames");

            merge_handle.join().unwrap();

            let _ffmpeg = ffmpeg_path.clone();
            let _inpt = current_exe_path
                .parent()
                .unwrap()
                .join(format!("temp\\out_frames\\{}\\frame%08d.png", part_index))
                .into_os_string()
                .into_string()
                .unwrap();
            let _outpt = current_exe_path
                .parent()
                .unwrap()
                .join(format!("temp\\video_parts\\{}.mp4", part_index))
                .into_os_string()
                .into_string()
                .unwrap();
            let _frmrt = original_frame_rate.clone();
            let _crf = args.crf.clone().to_string();
            let _preset = args.preset.clone();
            let _x265_params = args.x265params.clone();

            let progress_bar = m.insert_after(&last_pb, ProgressBar::new(frame_number as u64));
            progress_bar.set_style(ProgressStyle::default_bar()
                        .template("[merg][{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos:>7}/{len:7} merging segment          {per_sec:<12}")
                        .unwrap().progress_chars("#>-"));
            last_pb = progress_bar.clone();

            merge_handle = thread::spawn(move || {
                fs::remove_dir_all(&inpt_dir).unwrap();
                merge_frames(
                    &_ffmpeg,
                    &_inpt,
                    &_outpt,
                    &_frmrt,
                    &_crf,
                    &_preset,
                    &_x265_params,
                    progress_bar,
                )
                .unwrap();
                fs::remove_dir_all(&outpt_dir).unwrap();
            });
            unprocessed_indexes.remove(0);

            pb.set_position((parts_num - unprocessed_indexes.len() as i32 - 1) as u64);
        }
        merge_handle.join().unwrap();
    }

    // Merge video parts
    let mut f_content = format!(
        "file '{}'",
        current_exe_path
            .parent()
            .unwrap()
            .join("temp\\video_parts\\0.mp4")
            .into_os_string()
            .into_string()
            .unwrap()
    );

    for part_number in 1..parts_num {
        let video_part_path = current_exe_path
            .parent()
            .unwrap()
            .join(format!("temp\\video_parts\\{}.mp4", part_number))
            .into_os_string()
            .into_string()
            .unwrap();
        f_content = format!("{}\nfile '{}'", f_content, video_part_path);
    }

    fs::write(&txt_list_path, f_content).expect("Unable to write file");

    println!("merging video segments");
    let temp_video_path = current_exe_path
        .parent()
        .unwrap()
        .join("temp\\temp.mp4")
        .into_os_string()
        .into_string()
        .unwrap();

    {
        let mut count = 0;
        let p = Path::new(&temp_video_path);
        loop {
            thread::sleep(Duration::from_secs(1));
            if count == 5 {
                panic!("could not merge segments")
            } else if p.exists() {
                if std::fs::File::open(p).unwrap().metadata().unwrap().len() == 0 {
                    count += 1;
                } else {
                    break;
                }
            } else {
                merge_video_parts(&ffmpeg_path, &txt_list_path, &temp_video_path);
                count += 1;
            }
        }
    }

    println!("copying streams");
    copy_streams(
        &ffmpeg_path,
        &temp_video_path,
        &args.inputpath,
        &args.outputpath,
    );

    // Validation
    {
        let p = Path::new(&temp_video_path);
        if p.exists() && std::fs::File::open(p).unwrap().metadata().unwrap().len() != 0 {
            clear_dirs(&[&tmp_frames_path, &out_frames_path, &video_parts_path]);
            fs::remove_file(&txt_list_path).expect("Unable to delete file");
            fs::remove_file(&args_path).expect("Unable to delete file");
            fs::remove_file(&temp_video_path).expect("Unable to delete file");
        } else {
            panic!("final file validation error: try running again")
        }
    }

    clear().expect("failed to clear screen");
    println!("done");
}

fn get_frame_count(bin_path: &String, input_path: &String) -> i32 {
    let output = Command::new(bin_path)
        .arg("--Output=Video;%FrameCount%")
        .arg(input_path)
        .output()
        .expect("failed to execute process");
    let r = String::from_utf8(output.stdout)
        .unwrap()
        .trim()
        .parse::<i32>();
    match r {
        Err(_e) => 0,
        _ => r.unwrap(),
    }
}

fn get_frame_rate(bin_path: &String, input_path: &String) -> String {
    let output = Command::new(bin_path)
        .arg("--Output=Video;%FrameRate%")
        .arg(input_path)
        .output()
        .expect("failed to execute process");
    return String::from_utf8(output.stdout).unwrap().trim().to_string();
}

fn export_frames(
    bin_path: &String,
    input_path: &String,
    output_path: &String,
    start_time: &String,
    frame_number: &u32,
    progress_bar: ProgressBar,
) -> Result<(), Error> {
    let stderr = Command::new(bin_path)
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
    bin_path: &String,
    input_path: &String,
    output_path: &String,
    scale: &String,
    progress_bar: ProgressBar,
) -> Result<(), Error> {
    let stderr = Command::new(bin_path)
        .args([
            "-i",
            input_path,
            "-o",
            output_path,
            "-n",
            "realesr-animevideov3",
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

fn merge_frames(
    bin_path: &String,
    input_path: &String,
    output_path: &String,
    frame_rate: &String,
    crf: &String,
    preset: &String,
    x265_params: &String,
    progress_bar: ProgressBar,
) -> Result<(), Error> {
    let stderr = Command::new(bin_path)
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
            "libx265",
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

fn merge_video_parts(
    bin_path: &String,
    input_path: &String,
    output_path: &String,
) -> std::process::Output {
    Command::new(bin_path)
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
    bin_path: &String,
    video_input_path: &String,
    copy_input_path: &String,
    output_path: &String,
) -> std::process::Output {
    Command::new(bin_path)
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

fn clear_dirs(dirs: &[&str]) {
    for dir in dirs {
        match fs::remove_dir_all(dir) {
            Ok(_) => (),
            Err(_) => fs::remove_dir_all(dir).unwrap(),
        };
        fs::create_dir(dir).unwrap();
    }
}
