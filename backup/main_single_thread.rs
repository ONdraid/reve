use std::fs;
use std::time::Duration;
use std::env;
use std::thread;
use std::io::ErrorKind;
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Serialize, Deserialize};
use walkdir::WalkDir;
use clap::Parser;
use std::path::Path;
use std::process::Command;
use clearscreen;
use num_cpus;
use crossterm::cursor;
use colored::Colorize;

#[derive(Parser, Serialize, Deserialize, Debug)]
#[clap(author = "ONdraid", version = "v1", about = "Real-ESRGAN upscaler for longer videos with resume capability.", long_about = None)]
struct Args {
    /// Input video path (mp4/mkv)
    #[clap(short = 'i', long, value_parser)]
    inputpath: String,

    /// Output video path
    #[clap(short = 'o', long, value_parser)]
    outputpath: String,

    /// Part sizes (in frames)
    #[clap(short = 'p', long, value_parser, default_value_t = 1000)]
    partsize: u32,

    /// Video codec
    #[clap(short = 'c', long, value_parser, default_value = "h264_nvenc")]
    videoencoder: String,

    /// Video bitrate
    #[clap(short = 'b', long, value_parser, default_value = "12M")]
    videobitrate: String,

    /// Ffmpeg cpu utilization (full/half/quarter)
    #[clap(short = 't', long, value_parser, default_value = "full")]
    threads: String,
}

fn main() {
    let current_exe_path = env::current_exe().unwrap();
    let tmp_frames = current_exe_path.parent().unwrap().join("temp\\tmp_frames\\frame%08d.png").into_os_string().into_string().unwrap(); 
    let tmp_frames_path = current_exe_path.parent().unwrap().join("temp\\tmp_frames\\").into_os_string().into_string().unwrap();

    let out_frames = current_exe_path.parent().unwrap().join("temp\\out_frames\\frame%08d.jpg").into_os_string().into_string().unwrap();
    let out_frames_path = current_exe_path.parent().unwrap().join("temp\\out_frames\\").into_os_string().into_string().unwrap();

    let video_parts_path = current_exe_path.parent().unwrap().join("temp\\video_parts\\").into_os_string().into_string().unwrap();

    let args_path = current_exe_path.parent().unwrap().join("temp\\args.temp").into_os_string().into_string().unwrap();
    let txt_list_path = current_exe_path.parent().unwrap().join("temp\\parts.txt").into_os_string().into_string().unwrap();

    let args;
    let start_from;
    if Path::new(&args_path).exists() {
        clearscreen::clear().expect("failed to clear screen");
        let mut line = String::new();
        println!("{}\n{}", format!("Found existing temporary files.").red(), format!("Resume upscaling previous video (y/n)?"));
        std::io::stdin().read_line(&mut line).unwrap();
        line = line.trim().to_string();
        if line.to_lowercase().eq("n") | line.to_lowercase().eq("no") {
            clear_dirs(&[&tmp_frames_path, &out_frames_path, &video_parts_path]);
            match fs::remove_file(&txt_list_path) {
                Ok(()) => "ok",
                Err(_e) if _e.kind() == ErrorKind::NotFound => "not found",
                Err(_e) => "other",
            };
            let temp_video_path = current_exe_path.parent().unwrap().join("temp\\temp.mp4").into_os_string().into_string().unwrap();
            match fs::remove_file(&temp_video_path) {
                Ok(()) => "ok",
                Err(_e) if _e.kind() == ErrorKind::NotFound => "not found",
                Err(_e) => "other",
            };
            fs::remove_file(&args_path).expect("Unable to delete file");
            args = Args::parse();
            let serialized_args = serde_json::to_string(&args).unwrap();
            start_from = 0;
            fs::write(&args_path, serialized_args).expect("Unable to write file");
            clearscreen::clear().expect("failed to clear screen");
            show_intro();
            println!("{}", format!("Deleted all temporary files. Parsing console input.").green());
        } else if line.to_lowercase().eq("y") | line.to_lowercase().eq("yes") {
            let args_json = fs::read_to_string(&args_path).expect("Unable to read file");
            args = serde_json::from_str(&args_json).unwrap();
            start_from = WalkDir::new(&video_parts_path).into_iter().count() as i32 - 1;
            clear_dirs(&[&tmp_frames_path, &out_frames_path]);
            clearscreen::clear().expect("failed to clear screen");
            show_intro();
            println!("{}", format!("Resuming upscale. Next step: {}", start_from + 1).green());
        } else {
            clearscreen::clear().expect("failed to clear screen");
            panic!("Invalid answer. Expected yes/no (y/n).");
        }
    } else {
        start_from = 0;
        args = Args::parse();
        clear_dirs(&[&tmp_frames_path, &out_frames_path, &video_parts_path]);
        let serialized_args = serde_json::to_string(&args).unwrap();
        fs::write(&args_path, serialized_args).expect("Unable to write file");
        show_intro();
    }

    let ffmpeg_path = current_exe_path.parent().unwrap().join("bin\\ffmpeg").into_os_string().into_string().unwrap();
    let mediainfo_path = current_exe_path.parent().unwrap().join("bin\\mediainfo").into_os_string().into_string().unwrap();
    let realesrgan_path = current_exe_path.parent().unwrap().join("bin\\realesrgan-ncnn-vulkan").into_os_string().into_string().unwrap();


    // Checks
    {
    if !Path::new(&args.inputpath).exists() {
        panic!("Invalid input path.")
    }
    let output_path = Path::new(&args.outputpath);
    if output_path.exists() {
        panic!("Output path already exists.")
    }
    let extension = output_path.extension().unwrap();
    if extension != "mkv" {
        panic!("Incorrect output format {:?}. Currently only .mkv is supported.", extension);
    }
    }

    let threads: String;
    let cpu_count = num_cpus::get();
    if cpu_count == 1 {
        threads = "1".to_owned();
    } else if args.threads == "full" {
        threads = cpu_count.to_string();
    } else if args.threads == "half" {
        threads = (cpu_count as f64 / 2.0).ceil().to_string();
    } else if args.threads == "quarter" {
        threads = (cpu_count as f64 / 4.0).ceil().to_string();
    } else {
        panic!("Incorrect thread count argument {:?}. Expected full/half/quarter.", args.threads);
    }

    let total_frame_count = get_frame_count(&mediainfo_path, &args.inputpath);
    // println!("Total frame count: {}", total_frame_count);

    let original_frame_rate = get_frame_rate(&mediainfo_path, &args.inputpath);

    // Calculate steps
    let parts_num = (total_frame_count as f32 / args.partsize as f32).ceil() as i32;
    let last_part_size = total_frame_count % args.partsize as i32;
    println!("{}", format!("Total steps: {}, last step size: {}", parts_num, last_part_size).bright_red());
    
    for part_num in start_from..(parts_num - 1) {
        let start_frame = part_num * args.partsize as i32;

        println!("[{}/{}] Processing frames from {}", part_num + 1, parts_num, start_frame);

        // Export frames
        let ffmpeg_path_borrowed= ffmpeg_path.to_owned().clone();
        let temp_inputpath= args.inputpath.to_owned().clone();
        let tmp_frames_borrowed= tmp_frames.to_owned().clone();
        let threads_borrowed= threads.to_owned().clone();
        thread::spawn(move || {
             let _output = export_frames(&ffmpeg_path_borrowed, &temp_inputpath, &tmp_frames_borrowed, &start_frame, &args.partsize, &threads_borrowed);
        });

        let mut frame_count: u64 = 0;
        let progress_bar = ProgressBar::new(args.partsize as u64);
        progress_bar.set_style(ProgressStyle::default_bar()
                    .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} Exporting frames ({per_sec}, {eta})")
                    .progress_chars("#>-"));

        while frame_count != args.partsize as u64 {
            frame_count = WalkDir::new(&tmp_frames_path).into_iter().count() as u64 - 1;
            progress_bar.set_position(frame_count);
            thread::sleep(Duration::from_millis(500));
        }
        progress_bar.finish_and_clear();


        // Upscale frames
        let realesrgan_path_borrowed= realesrgan_path.to_owned().clone();
        let tmp_frames_path_borrowed= tmp_frames_path.to_owned().clone();
        let out_frames_path_borrowed= out_frames_path.to_owned().clone();
        thread::spawn(move || {
             let _output = upscale_frames(&realesrgan_path_borrowed, &tmp_frames_path_borrowed, &out_frames_path_borrowed);
        });

        let mut frame_count: u64 = 0;
        let progress_bar = ProgressBar::new(args.partsize as u64);
        progress_bar.set_style(ProgressStyle::default_bar()
                    .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} Upscaling frames ({per_sec}, {eta})")
                    .progress_chars("#>-"));
        while frame_count != args.partsize as u64 {
            frame_count = WalkDir::new(&out_frames_path).into_iter().count() as u64 - 1;
            progress_bar.set_position(frame_count);
            thread::sleep(Duration::from_millis(500));
        }
        progress_bar.finish_and_clear();


        // Merge frames
        println!("Merging frames");
        let video_part_path = current_exe_path.parent().unwrap().join(format!("temp\\video_parts\\{}.mp4", part_num)).into_os_string().into_string().unwrap();

        let _output = merge_frames(&ffmpeg_path, &out_frames, &video_part_path, &original_frame_rate, &args.videoencoder, &args.videobitrate);
        
        clear_dirs(&[&tmp_frames_path, &out_frames_path]);

        clearscreen::clear().expect("failed to clear screen");
    }


    // Last step
    let start_frame = (parts_num - 1) * args.partsize as i32;
    let count: u64;

    // Export Frames
    println!("[{}/{}] Processing frames from {}", parts_num, parts_num, start_frame);
    if last_part_size != 0 {
        count = last_part_size as u64;
        let ffmpeg_path_borrowed= ffmpeg_path.to_owned().clone();
        let temp_inputpath= args.inputpath.to_owned().clone();
        let tmp_frames_borrowed= tmp_frames.to_owned().clone();
        thread::spawn(move || {
             let _output = export_frames(&ffmpeg_path_borrowed, &temp_inputpath, &tmp_frames_borrowed, &start_frame, &(last_part_size as u32), &threads);
        });
    }
    else {
        count = args.partsize as u64;
        let ffmpeg_path_borrowed= ffmpeg_path.to_owned().clone();
        let temp_inputpath= args.inputpath.to_owned().clone();
        let tmp_frames_borrowed= tmp_frames.to_owned().clone();
        thread::spawn(move || {
             let _output = export_frames(&ffmpeg_path_borrowed, &temp_inputpath, &tmp_frames_borrowed, &start_frame, &args.partsize, &threads);
        });
    }

    let mut frame_count: u64 = 0;
        let progress_bar = ProgressBar::new(count);
        progress_bar.set_style(ProgressStyle::default_bar()
                    .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} Exporting frames ({per_sec}, {eta})")
                    .progress_chars("#>-"));

        while frame_count != count as u64 {
            frame_count = WalkDir::new(&tmp_frames_path).into_iter().count() as u64 - 1;
            progress_bar.set_position(frame_count);
            thread::sleep(Duration::from_millis(500));
        }
        progress_bar.finish_and_clear();


    // Upscale frames
    let realesrgan_path_borrowed= realesrgan_path.to_owned().clone();
    let tmp_frames_path_borrowed= tmp_frames_path.to_owned().clone();
    let out_frames_path_borrowed= out_frames_path.to_owned().clone();
    thread::spawn(move || {
         let _output = upscale_frames(&realesrgan_path_borrowed, &tmp_frames_path_borrowed, &out_frames_path_borrowed);
    });

    let mut frame_count: u64 = 0;
    let progress_bar = ProgressBar::new(count);
    progress_bar.set_style(ProgressStyle::default_bar()
                .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} Upscaling frames ({per_sec}, {eta})")
                .progress_chars("#>-"));
    while frame_count != count as u64 {
        frame_count = WalkDir::new(&out_frames_path).into_iter().count() as u64 - 1;
        progress_bar.set_position(frame_count);
        thread::sleep(Duration::from_millis(500));
    }
    progress_bar.finish_and_clear();


    // Merge frames
    println!("Merging frames");
    let video_part_path = current_exe_path.parent().unwrap().join(format!("temp\\video_parts\\{}.mp4", parts_num - 1)).into_os_string().into_string().unwrap();
    let _output = merge_frames(&ffmpeg_path, &out_frames, &video_part_path, &original_frame_rate, &args.videoencoder, &args.videobitrate);

    clear_dirs(&[&tmp_frames_path, &out_frames_path]);
    

    // Merge video parts
    let mut f_content = format!("file '{}'", current_exe_path.parent().unwrap().join("temp\\video_parts\\0.mp4").into_os_string().into_string().unwrap());

    for part_number in 1..parts_num {
        let video_part_path = current_exe_path.parent().unwrap().join(format!("temp\\video_parts\\{}.mp4", part_number)).into_os_string().into_string().unwrap();
        f_content = format!("{}\nfile '{}'", f_content, video_part_path);
    }

    fs::write(&txt_list_path, f_content).expect("Unable to write file");

    println!("Merging video parts");
    let temp_video_path = current_exe_path.parent().unwrap().join("temp\\temp.mp4").into_os_string().into_string().unwrap();
    merge_video_parts(&ffmpeg_path, &txt_list_path, &temp_video_path);

    println!("Copying streams");
    copy_streams(&ffmpeg_path, &temp_video_path, &args.inputpath, &args.outputpath);

    clear_dirs(&[&tmp_frames_path, &out_frames_path, &video_parts_path]);
    fs::remove_file(&txt_list_path).expect("Unable to delete file");
    fs::remove_file(&args_path).expect("Unable to delete file");
    fs::remove_file(&temp_video_path).expect("Unable to delete file");

    clearscreen::clear().expect("failed to clear screen");
    print!("Done");
}

fn show_intro() {
    clearscreen::clear().expect("failed to clear screen");
    let to_print = r" ______     ______     __   __   ______     ______     __    
/\  == \   /\  ___\   /\ \ / /  /\  ___\   /\  __ \   /\ \   
\ \  __<   \ \  __\   \ \ \'/   \ \  __\   \ \  __ \  \ \ \  
 \ \_\ \_\  \ \_____\  \ \__|    \ \_____\  \ \_\ \_\  \ \_\ 
  \/_/ /_/   \/_____/   \/_/      \/_____/   \/_/\/_/   \/_/ 
                                                             
    ";
    cursor().hide().expect("Unable to hide cursor");
    for _ in 0..4 {
    print!("{}", format!("{to_print}").blue());
    thread::sleep(Duration::from_millis(200));
    cursor().move_up(6);
    cursor().move_left(4);

    print!("{}", format!("{to_print}").cyan());
    thread::sleep(Duration::from_millis(200));
    cursor().move_up(6);
    cursor().move_left(4);
    }
    cursor().show().expect("Unable to show cursor");
    clearscreen::clear().expect("failed to clear screen");
}

fn get_frame_count(bin_path: &String, input_path: &String) -> i32 {
    let output = Command::new(bin_path)
                    .arg("--Output=Video;%FrameCount%")
                    .arg(input_path)
                    .output()
                    .expect("failed to execute process");
    return String::from_utf8(output.stdout).unwrap().trim().parse::<i32>().unwrap();
}

fn get_frame_rate(bin_path: &String, input_path: &String) -> String {
    let output = Command::new(bin_path)
                    .arg("--Output=Video;%FrameRate%")
                    .arg(input_path)
                    .output()
                    .expect("failed to execute process");
    return String::from_utf8(output.stdout).unwrap().trim().to_string();
}

// "./bin/ffmpeg" -i video.mkv -qmin 1 -qmax 1 -vf "select=gte(n\,0)" -vsync 0 -vframes 1000 "./temp/tmp_frames/frame%08d.png"

fn export_frames(bin_path: &String, input_path: &String, output_path: &String, start_frame: &i32, frame_number: &u32, threads: &String) -> std::process::Output {
    Command::new(bin_path)
            .args([
                "-i", input_path,
                "-qscale:v", "1",
                "-qmin", "1",
                "-qmax", "1",
                "-vf", &format!("select='gte(n\\,{})'", start_frame).to_owned(),
                "-vsync", "0",
                "-vframes", &frame_number.to_string(),
                "-threads", threads,
                output_path])
            .output()
            .expect("failed to execute process")
}

fn upscale_frames(bin_path: &String, input_path: &String, output_path: &String) -> std::process::Output {
    Command::new(bin_path)
            .args([
                "-i", input_path,
                "-o", output_path,
                "-n", "realesr-animevideov3",
                "-s", "2",
                "-f", "jpg"])
            .output()
            .expect("failed to execute process")
}

fn merge_frames(bin_path: &String, input_path: &String, output_path: &String, frame_rate: &String, encoder: &String, bitrate: &String) -> std::process::Output {
    Command::new(bin_path)
            .args([
                "-r", frame_rate,
                "-i", input_path,
                "-c:v", encoder,
                "-b:v", bitrate,
                output_path])
            .output()
            .expect("failed to execute process")
}

fn merge_video_parts(bin_path: &String, input_path: &String , output_path: &String) -> std::process::Output {
    Command::new(bin_path)
            .args([
                "-f", "concat",
                "-safe", "0",
                "-i", input_path,
                "-c", "copy",
                output_path])
            .output()
            .expect("failed to execute process")
}

// "./bin/ffmpeg" -i output.mkv -vn -i video.mkv -c copy -map 0:v -map 1 out.mkv

fn copy_streams(bin_path: &String, video_input_path: &String, copy_input_path: &String, output_path: &String) -> std::process::Output {
    Command::new(bin_path)
            .args([
                "-i", video_input_path,
                "-vn", "-i", copy_input_path,
                "-c", "copy",
                "-map", "0:v",
                "-map", "1",
                output_path])
            .output()
            .expect("failed to execute process")
}

fn clear_dirs(dirs: &[&str]) {
    for dir in dirs {
        fs::remove_dir_all(dir).unwrap();
        fs::create_dir(dir).unwrap();
    }
}
