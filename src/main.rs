use std::fs;
use std::env;
use std::str::FromStr;
use std::thread;
use std::io::ErrorKind;
use colored::Colorize;
use serde::{Serialize, Deserialize};
use walkdir::WalkDir;
use clap::Parser;
use std::path::Path;
use std::process::Command;
use clearscreen::clear;

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
    #[clap(short = 'c', long, value_parser = clap::value_parser!(u8).range(0..52), default_value_t = 18)]
    crf: u8,

    /// video encoding preset
    #[clap(short = 'p', long, value_parser = preset_validation, default_value = "slow")]
    preset: String,

    /// x265 encoding parameters
    #[clap(short = 'x', long, value_parser, default_value = "limit-sao:bframes=8:psy-rd=1.5:psy-rdoq=2:aq-mode=3")]
    x265params: String,

    /// enable safe mode
    #[clap(short = 'S', long, action)]
    safemode: bool,
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
        "ultrafast" | "superfast" | "veryfast" | "faster" | "fast" | "medium" | "slow" | "slower" | "veryslow" => Ok(s.to_string()),
        _ => Err(String::from_str("valid: ultrafast/superfast/veryfast/faster/fast/medium/slow/slower/veryslow").unwrap())
    }
}

fn main() {
    let current_exe_path = env::current_exe().unwrap();

    let tmp_frames_path = current_exe_path.parent().unwrap().join("temp\\tmp_frames\\").into_os_string().into_string().unwrap();
    let out_frames_path = current_exe_path.parent().unwrap().join("temp\\out_frames\\").into_os_string().into_string().unwrap();
    let video_parts_path = current_exe_path.parent().unwrap().join("temp\\video_parts\\").into_os_string().into_string().unwrap();

    let args_path = current_exe_path.parent().unwrap().join("temp\\args.temp").into_os_string().into_string().unwrap();
    let txt_list_path = current_exe_path.parent().unwrap().join("temp\\parts.txt").into_os_string().into_string().unwrap();

    let args;
    if Path::new(&args_path).exists() {
        clear().expect("failed to clear screen");
        let mut line = String::new();
        println!("{}\n{}", format!("found existing temporary files.").red(), format!("resume upscaling previous video (y/n)?"));
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
            fs::write(&args_path, serialized_args).expect("Unable to write file");
            clear().expect("failed to clear screen");
            println!("{}", format!("deleted all temporary files, parsing console input").green());
        } else if line.to_lowercase().eq("y") | line.to_lowercase().eq("yes") {
            let args_json = fs::read_to_string(&args_path).expect("Unable to read file");
            args = serde_json::from_str(&args_json).unwrap();
            clear_dirs(&[&tmp_frames_path, &out_frames_path]);
            clear().expect("failed to clear screen");
            println!("{}", format!("resuming upscale").green());
        } else {
            clear().expect("failed to clear screen");
            panic!("invalid answer. expected y/n.");
        }
    } else {
        args = Args::parse();
        clear_dirs(&[&tmp_frames_path, &out_frames_path, &video_parts_path]);
        let serialized_args = serde_json::to_string(&args).unwrap();
        fs::write(&args_path, serialized_args).expect("Unable to write file");
    }

    let ffmpeg_path = current_exe_path.parent().unwrap().join("bin\\ffmpeg").into_os_string().into_string().unwrap();
    let mediainfo_path = current_exe_path.parent().unwrap().join("bin\\mediainfo").into_os_string().into_string().unwrap();
    let realesrgan_path = current_exe_path.parent().unwrap().join("bin\\realesrgan-ncnn-vulkan").into_os_string().into_string().unwrap();


    // Validation
    {
        let in_extension = Path::new(&args.inputpath).extension().unwrap();
        let out_extension = Path::new(&args.outputpath).extension().unwrap();

        if in_extension == "mkv" && out_extension != "mkv" {
            clear().expect("failed to clear screen");
            println!("{}{}{}{}{}{}\n\n{}{}",
                format!("error").bright_red(),
                format!(" Invalid value "),
                format!("\"{}\"", args.inputpath).yellow(),
                format!(" for '"),
                format!("--outputpath <OUTPUTPATH>").yellow(),
                format!("': mkv file can only be exported as mkv file"),
                format!("For more information try "),
                format!("--help").green());
            std::process::exit(1);
        }
    }

    let total_frame_count = get_frame_count(&mediainfo_path, &args.inputpath);
    let original_frame_rate = get_frame_rate(&mediainfo_path, &args.inputpath);

    // Calculate steps
    let parts_num = (total_frame_count as f32 / args.segmentsize as f32).ceil() as i32;
    let last_part_size = total_frame_count % args.segmentsize as i32;
    clear().expect("failed to clear screen");
    println!("{}", format!("total segments: {}, last segment size: {}", parts_num, last_part_size).red());


    let mut unprocessed_indexes= Vec::new();
    for i in 0..parts_num {
        let n = format!("{}\\{}.mp4", video_parts_path, i);
        let p = Path::new(&n);
        if !p.exists() {
            unprocessed_indexes.push(i);
        } else {
            let c = get_frame_count(&mediainfo_path, &p.display().to_string());
            if c != args.segmentsize as i32 {
                fs::remove_file(p).expect("could not remove invalid part, maybe in use?");
                print!("removed invalid segment file [{}] with {} frame size\n", i, c);
                unprocessed_indexes.push(i);
            }
        }
    }


    let mut join_handlers = Vec::new();
    let mut merge_handle= thread::spawn(||{});

    while unprocessed_indexes.len() != 0 {
        let part_index = unprocessed_indexes[0];
        while join_handlers.len() != 3 && join_handlers.len() < unprocessed_indexes.len() {
            let index = unprocessed_indexes[join_handlers.len()];
            let _ffmpeg = ffmpeg_path.clone();
            let _inpt = args.inputpath.clone();
            let _outpt = current_exe_path.parent().unwrap().join(format!("temp\\tmp_frames\\{}\\frame%08d.png", index)).into_os_string().into_string().unwrap();
            let _start_frame = index * args.segmentsize as i32;
            let _index_dir = current_exe_path.parent().unwrap().join(format!("temp\\tmp_frames\\{}", index)).into_os_string().into_string().unwrap();
            let _frame_number;
            if index + 1 == parts_num && last_part_size != 0 {
                _frame_number = last_part_size;
            } else {
                _frame_number = args.segmentsize as i32;
            }
            print!("exporting segment [{}], start frame: {}, frame number: {}\n", index, _start_frame, _frame_number);
            let thread_join_handle = thread::spawn(move || {
                fs::create_dir(&_index_dir).expect("could not create directory");
                export_frames(&_ffmpeg, &_inpt, &_outpt, &_start_frame, &(_frame_number as u32));
            });
            join_handlers.push(thread_join_handle);
        }

        let inpt_dir = current_exe_path.parent().unwrap().join(format!("temp\\tmp_frames\\{}", part_index)).into_os_string().into_string().unwrap();
        let outpt_dir = current_exe_path.parent().unwrap().join(format!("temp\\out_frames\\{}", part_index)).into_os_string().into_string().unwrap();

        if WalkDir::new(&inpt_dir).into_iter().count() as i32 - 1 != args.segmentsize as i32 {
            print!("waiting for export...\n");   
        }

        join_handlers.remove(0).join().expect("could not handle thread");
        if args.safemode {
            print!("waiting for merge...\n"); 
            merge_handle.join().expect("could not handle thread");
        }
        fs::create_dir(&outpt_dir).expect("could not create directory");
        print!("upscaling segment [{}]\n", part_index);
        upscale_frames(&realesrgan_path, &inpt_dir, &outpt_dir, &args.scale.to_string());
        
        let _ffmpeg = ffmpeg_path.clone();
        let _inpt = current_exe_path.parent().unwrap().join(format!("temp\\out_frames\\{}\\frame%08d.jpg", part_index)).into_os_string().into_string().unwrap();
        let _outpt = current_exe_path.parent().unwrap().join(format!("temp\\video_parts\\{}.mp4", part_index)).into_os_string().into_string().unwrap();
        let _frmrt = original_frame_rate.clone();
        let _crf = args.crf.clone().to_string();
        let _preset = args.preset.clone();
        let _x265_params = args.x265params.clone();
        print!("merging segment [{}]\n", part_index);
        merge_handle = thread::spawn(move || {
            fs::remove_dir_all(&inpt_dir).unwrap();
            merge_frames(&_ffmpeg, &_inpt, &_outpt, &_frmrt, &_crf, &_preset, &_x265_params);
            fs::remove_dir_all(&outpt_dir).unwrap();
        });
        unprocessed_indexes.remove(0);
    }
    merge_handle.join().expect("could not handle thread");


    // Merge video parts
    let mut f_content = format!("file '{}'", current_exe_path.parent().unwrap().join("temp\\video_parts\\0.mp4").into_os_string().into_string().unwrap());

    for part_number in 1..parts_num {
        let video_part_path = current_exe_path.parent().unwrap().join(format!("temp\\video_parts\\{}.mp4", part_number)).into_os_string().into_string().unwrap();
        f_content = format!("{}\nfile '{}'", f_content, video_part_path);
    }

    fs::write(&txt_list_path, f_content).expect("Unable to write file");

    println!("merging video segments");
    let temp_video_path = current_exe_path.parent().unwrap().join("temp\\temp.mp4").into_os_string().into_string().unwrap();
    merge_video_parts(&ffmpeg_path, &txt_list_path, &temp_video_path);

    println!("copying streams");
    copy_streams(&ffmpeg_path, &temp_video_path, &args.inputpath, &args.outputpath);

    clear_dirs(&[&tmp_frames_path, &out_frames_path, &video_parts_path]);
    fs::remove_file(&txt_list_path).expect("Unable to delete file");
    fs::remove_file(&args_path).expect("Unable to delete file");
    fs::remove_file(&temp_video_path).expect("Unable to delete file");

    clear().expect("failed to clear screen");
    print!("done");
}


fn get_frame_count(bin_path: &String, input_path: &String) -> i32 {
    let output = Command::new(bin_path)
                    .arg("--Output=Video;%FrameCount%")
                    .arg(input_path)
                    .output()
                    .expect("failed to execute process");
    let r = String::from_utf8(output.stdout).unwrap().trim().parse::<i32>();
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

fn export_frames(bin_path: &String, input_path: &String, output_path: &String, start_frame: &i32, frame_number: &u32) -> std::process::Output {
    Command::new(bin_path)
            .args([
                "-i", input_path,
                "-qscale:v", "1",
                "-qmin", "1",
                "-qmax", "1",
                "-vf", &format!("select='gte(n\\,{})'", start_frame),
                "-vsync", "0",
                "-vframes", &frame_number.to_string(),
                output_path])
            .output()
            .expect("failed to execute process")
}

fn upscale_frames(bin_path: &String, input_path: &String, output_path: &String, scale: &String) -> std::process::Output {
    Command::new(bin_path)
            .args([
                "-i", input_path,
                "-o", output_path,
                "-n", "realesr-animevideov3",
                "-s", scale,
                "-f", "jpg"])
            .output()
            .expect("failed to execute process")
}


// "./bin/ffmpeg" -f image2 -framerate 23.976/1 -i 0/frame%08d.jpg -c:v libx265 -pix_fmt yuv420p10le -crf 18 -preset slow -x265-params limit-sao:bframes=8:psy-rd=1.5:psy-rdoq=2:aq-mode=3 out.mp4

fn merge_frames(bin_path: &String, input_path: &String, output_path: &String, frame_rate: &String, crf: &String, preset: &String, x265_params: &String) -> std::process::Output {
    Command::new(bin_path)
            .args([
                "-f", "image2",
                "-framerate", &format!("{}/1", frame_rate),
                "-i", input_path,
                "-c:v", "libx265",
                "-pix_fmt", "yuv420p10le",
                "-crf", crf,
                "-preset", preset,
                "-x265-params", x265_params,
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
