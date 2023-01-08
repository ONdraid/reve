use clap::Parser;
use clearscreen::clear;
use colored::Colorize;
use dialoguer::Confirm;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use path_clean::PathClean;
use reve_shared::*;
use std::env;
use std::fs;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::thread;

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

fn main() {
    let current_exe_path = env::current_exe().unwrap();

    let args_path = current_exe_path
        .parent()
        .unwrap()
        .join("temp\\args.temp")
        .into_os_string()
        .into_string()
        .unwrap();

    let mut args;
    let mut video;
    if Path::new(&args_path).exists() {
        clear().unwrap();
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
            args.inputpath = absolute_path(PathBuf::from_str(&args.inputpath).unwrap());
            println!("{} loaded", args.inputpath);
            args.outputpath = absolute_path(PathBuf::from_str(&args.outputpath).unwrap());

            env::set_current_dir(current_exe_path.parent().unwrap()).unwrap();
            rebuild_temp(false);

            let serialized_args = serde_json::to_string(&args).unwrap();
            fs::write(&args_path, serialized_args).expect("Unable to write file");
            video = Video::new(
                &args.inputpath,
                &args.outputpath,
                args.segmentsize,
                args.scale,
            );
            let serialized_video = serde_json::to_string(&video).unwrap();
            fs::write("temp\\video.temp", serialized_video).unwrap();
            clear().unwrap();
            println!(
                "{}",
                "deleted all temporary files, parsing console input"
                    .to_string()
                    .green()
            );
        } else {
            // Resume upscale
            env::set_current_dir(current_exe_path.parent().unwrap()).unwrap();
            let args_json = fs::read_to_string(&args_path).unwrap();
            args = serde_json::from_str(&args_json).unwrap();
            let video_json = fs::read_to_string("temp\\video.temp").unwrap();
            video = serde_json::from_str(&video_json).unwrap();

            rebuild_temp(true);
            clear().unwrap();
            println!("{}", "resuming upscale".to_string().green());
        }
    } else {
        // Start new
        args = Args::parse();
        args.inputpath = absolute_path(PathBuf::from_str(&args.inputpath).unwrap());
        println!("{} loaded", args.inputpath);
        args.outputpath = absolute_path(PathBuf::from_str(&args.outputpath).unwrap());
        env::set_current_dir(current_exe_path.parent().unwrap()).unwrap();

        rebuild_temp(false);
        let serialized_args = serde_json::to_string(&args).unwrap();
        fs::write(&args_path, serialized_args).expect("Unable to write file");
        video = Video::new(
            &args.inputpath,
            &args.outputpath,
            args.segmentsize,
            args.scale,
        );
        let serialized_video = serde_json::to_string(&video).unwrap();
        fs::write("temp\\video.temp", serialized_video).unwrap();
    }

    // Validation
    {
        let in_extension = Path::new(&args.inputpath).extension().unwrap();
        let out_extension = Path::new(&args.outputpath).extension().unwrap();

        if in_extension == "mkv" && out_extension != "mkv" {
            clear().unwrap();
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

    if video.segments.is_empty() {
        video.segments.push(Segment {
            index: video.segment_count - 1,
            size: get_last_segment_size(video.frame_count, args.segmentsize),
        });
    } else if video.segments[0].index > 0 {
        video.segments.insert(
            0,
            Segment {
                index: video.segments[0].index - 1,
                size: args.segmentsize,
            },
        );
    }
    let _ = fs::remove_file(format!(
        "temp\\video_parts\\{}.mp4",
        video.segments[0].index
    ));

    clear().unwrap();
    println!(
        "{}",
        format!(
            "total segments: {}, last segment size: {} (ctrl+c to exit)",
            video.segment_count,
            video.segments.last().unwrap().size
        )
            .red()
    );

    {
        let mut export_handle = thread::spawn(move || {});
        let mut merge_handle = thread::spawn(move || {});
        let mut remove_handle = thread::spawn(move || {});
        let info_style = "[info][{elapsed_precise}] [{wide_bar:.green/white}] {pos:>7}/{len:7} processed segments       eta: {eta:<7}";
        let expo_style = "[expo][{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos:>7}/{len:7} exporting segment        {per_sec:<12}";
        let upsc_style = "[upsc][{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos:>7}/{len:7} upscaling segment        {per_sec:<12}";
        let merg_style = "[merg][{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos:>7}/{len:7} merging segment          {per_sec:<12}";

        let m = MultiProgress::new();
        let pb = m.add(ProgressBar::new(video.segment_count as u64));
        pb.set_style(
            ProgressStyle::default_bar()
                .template(info_style)
                .unwrap()
                .progress_chars("#>-"),
        );
        let mut last_pb = pb.clone();

        // Initial export
        if !video.segments.is_empty() {
            let index = video.segments[0].index;

            let progress_bar =
                m.insert_after(&last_pb, ProgressBar::new(video.segments[0].size as u64));
            progress_bar.set_style(
                ProgressStyle::default_bar()
                    .template(expo_style)
                    .unwrap()
                    .progress_chars("#>-"),
            );
            last_pb = progress_bar.clone();

            let reader = video.export_segment(index as usize).unwrap();
            let mut count: i32 = -1;
            reader
                .lines()
                .filter_map(|line| line.ok())
                .filter(|line| line.contains("AVIOContext"))
                .for_each(|_| {
                    count += 1;
                    progress_bar.set_position(count as u64);
                });
            m.clear().unwrap();
        }

        for _ in 0..video.segments.len() {
            export_handle.join().unwrap();
            if video.segments.len() == 1 {
                export_handle = thread::spawn(move || {});
            } else {
                let index = video.segments[1].index;

                let progress_bar =
                    m.insert_after(&last_pb, ProgressBar::new(video.segments[1].size as u64));
                progress_bar.set_style(
                    ProgressStyle::default_bar()
                        .template(expo_style)
                        .unwrap()
                        .progress_chars("#>-"),
                );
                last_pb = progress_bar.clone();

                let reader = video.export_segment(index as usize).unwrap();
                export_handle = thread::spawn(move || {
                    let mut count: i32 = -1;
                    reader
                        .lines()
                        .filter_map(|line| line.ok())
                        .filter(|line| line.contains("AVIOContext"))
                        .for_each(|_| {
                            count += 1;
                            progress_bar.set_position(count as u64);
                        });
                });
            }

            let input_directory = format!("temp\\tmp_frames\\{}", video.segments[0].index);

            {
                let progress_bar =
                    m.insert_after(&last_pb, ProgressBar::new(video.segments[0].size as u64));
                progress_bar.set_style(
                    ProgressStyle::default_bar()
                        .template(upsc_style)
                        .unwrap()
                        .progress_chars("#>-"),
                );
                last_pb = progress_bar.clone();

                let reader = video
                    .upscale_segment(video.segments[0].index as usize)
                    .unwrap();
                let mut count = 0;
                reader
                    .lines()
                    .filter_map(|line| line.ok())
                    .filter(|line| line.contains("done"))
                    .for_each(|_| {
                        count += 1;
                        progress_bar.set_position(count);
                    });
            }

            thread::spawn(move || {
                fs::remove_dir_all(&input_directory).unwrap();
            });

            merge_handle.join().unwrap();
            let path_to_remove =
                format!("temp\\out_frames\\{}", video.segments[0].index as i32 - 1);
            remove_handle = thread::spawn(move || {
                let _ = fs::remove_dir_all(&path_to_remove);
            });

            let progress_bar =
                m.insert_after(&last_pb, ProgressBar::new(video.segments[0].size as u64));
            progress_bar.set_style(
                ProgressStyle::default_bar()
                    .template(merg_style)
                    .unwrap()
                    .progress_chars("#>-"),
            );
            last_pb = progress_bar.clone();

            let input = format!(
                "temp\\out_frames\\{}\\frame%08d.png",
                video.segments[0].index
            );
            let output = format!("temp\\video_parts\\{}.mp4", video.segments[0].index);
            let frame_rate = format!("{}/1", video.frame_rate);
            let crf = args.crf.to_string();

            // TODO: move this away
            let args = vec![
                "-v",
                "verbose",
                "-f",
                "image2",
                "-framerate",
                &frame_rate,
                "-i",
                &input,
                "-c:v",
                "libx265",
                "-pix_fmt",
                "yuv420p10le",
                "-crf",
                &crf,
                "-preset",
                &args.preset,
                "-x265-params",
                &args.x265params,
                &output,
            ];

            let reader = video.merge_segment(args).unwrap();
            merge_handle = thread::spawn(move || {
                let mut count = 0;
                reader
                    .lines()
                    .filter_map(|line| line.ok())
                    .filter(|line| line.contains("AVIOContext"))
                    .for_each(|_| {
                        count += 1;
                        progress_bar.set_position(count);
                    });
            });
            video.segments.remove(0);

            let serialized_video = serde_json::to_string(&video).unwrap();
            fs::write("temp\\video.temp", serialized_video).unwrap();
            pb.set_position((video.segment_count - video.segments.len() as u32 - 1) as u64);
        }
        merge_handle.join().unwrap();
        remove_handle.join().unwrap();

        m.clear().unwrap();
    }

    println!("merging video segments");
    video.concatenate_segments();

    // Validation
    {
        let p = Path::new(&args.outputpath);
        if p.exists() && fs::File::open(p).unwrap().metadata().unwrap().len() != 0 {
            rebuild_temp(false);
        } else {
            panic!("final file validation error: try running again")
        }
    }

    println!("done!");
}
