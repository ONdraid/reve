use clap::Parser;
use clearscreen::clear;
use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use path_clean::PathClean;
use reve_shared::*;
use rusqlite::{params, Connection};
use std::env;
use std::fs;
use std::fs::metadata;
use std::io::ErrorKind;
use std::path::Path;
use std::process::exit;
use std::process::Command;
use std::thread;
use std::time::Duration;
use std::time::Instant;

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
    let main_now = Instant::now();

    let mut args;
    args = Args::parse();

    let temp_args;
    temp_args = Args::parse();

    #[cfg(target_os = "linux")]
    match dev_shm_exists() {
        Err(e) => {
            println!("{:?}", e);
            exit(1);
        }
        _ => (),
    };

    let mut output_path: String = "".to_string();
    let mut done_output: String = "".to_string();
    let mut current_file_count = 0;
    let mut total_files: i32;
    let resolution = temp_args.resolution.unwrap().parse::<u32>().unwrap();

    let md = metadata(Path::new(&args.inputpath)).unwrap();
    // Check if input is a directory, if yes, check how many video files are in it, and process the ones that are smaller than the given resolution
    if md.is_dir() {
        let mut count;
        let db_count;
        let db_count_added;
        let db_count_skipped;
        let walk_count: u64 = walk_count(&args.inputpath) as u64;
        let files_bar = ProgressBar::new(walk_count);
        let files_style = "[file][{elapsed_precise}] [{wide_bar:.green/white}] {percent}% {pos:>7}/{len:7} analyzed files       eta: {eta:<7}";
        files_bar.set_style(
            ProgressStyle::default_bar()
                .template(files_style)
                .unwrap()
                .progress_chars("#>-"),
        );

        let vector_files = walk_files(&args.inputpath);
        let mut vector_files_to_process_frames_count: Vec<u64> = Vec::new();

        let result = add_to_db(
            vector_files.clone(),
            // if some args.resolution is given, use it, if not, use 0
            resolution.clone().to_string(),
            files_bar.clone(),
        )
        .unwrap();
        // get the counters from the add_to_db function
        let counters = result.0;

        let to_process = result.1;
        // get the vector of files to process
        let mut vector_files_to_process = to_process.lock().unwrap().clone();

        // count, db_count, db_count_added, db_count_skipped
        count = format!("{:?}", counters[0]).parse::<i32>().unwrap();
        db_count = format!("{:?}", counters[1]).parse::<u64>().unwrap();
        db_count_added = format!("{:?}", counters[2]).parse::<u64>().unwrap();
        db_count_skipped = format!("{:?}", counters[3]).parse::<u64>().unwrap();

        if vector_files_to_process.len() == 0 {
            // get all the files from the database that contain input_path's folder parent in column filepath and status 'processing' in status column and add them to the vector_files_to_process
            let conn = Connection::open("reve.db").unwrap();
            let input = args.inputpath.clone();
            let mut stmt = conn
                .prepare("SELECT * FROM video_info WHERE status = 'processing' AND filepath LIKE ?")
                .unwrap();
            let mut rows = stmt.query(&[&format!("%{}%", input)]).unwrap();
            while let Some(row) = rows.next().unwrap() {
                vector_files_to_process.push(row.get(2).unwrap());
            }
            // get all the files from the database that contain input_path's folder parent in column filepath and status 'pending' in status column and add them to the vector_files_to_process
            let conn = Connection::open("reve.db").unwrap();
            let input = args.inputpath.clone();
            let mut stmt = conn
                .prepare("SELECT * FROM video_info WHERE status = 'pending' AND filepath LIKE ?")
                .unwrap();
            let mut rows = stmt.query(&[&format!("%{}%", input)]).unwrap();
            while let Some(row) = rows.next().unwrap() {
                vector_files_to_process.push(row.get(2).unwrap());
            }
        }

        if count == 0 && vector_files_to_process.len() != 0 {
            count = vector_files_to_process.len() as i32;
            current_file_count = db_count - vector_files_to_process.len() as u64;
        }

        let frame_count_bar = ProgressBar::new(vector_files_to_process.len() as u64);
        let frame_count_style = "[frm_cnt][{elapsed_precise}] [{wide_bar:.green/white}] {percent}% {pos:>7}/{len:7} counted frames       eta: {eta:<7}";
        frame_count_bar.set_style(
            ProgressStyle::default_bar()
                .template(frame_count_style)
                .unwrap()
                .progress_chars("#>-"),
        );

        files_bar.finish_and_clear();
        println!("Added {} files to the database ({} already present, {} skipped due to max resolution being {}p)", db_count_added, db_count, db_count_skipped, resolution);
        println!(
            "Upscaling {} files (Due to max height resolution: {}p)",
            count, resolution
        );

        let total_frames = vector_files_to_process.clone();
        let mut current_frame_count: u64 = 0;
        for file in total_frames.clone() {
            current_frame_count += u64::from(get_frame_count(&file));
            vector_files_to_process_frames_count.push(current_frame_count);
            frame_count_bar.inc(1);
        }

        if current_frame_count == 0 {
            vector_files_to_process_frames_count.clear();
            if vector_files_to_process_frames_count.is_empty() {
                for file in total_frames.clone() {
                    current_frame_count += u64::from(get_frame_count_tag(&file));
                    vector_files_to_process_frames_count.push(current_frame_count);
                }
            }
        }

        // current_frame_count = 0; then get the frame count by dividing the duration by the fps
        if current_frame_count == 0 {
            vector_files_to_process_frames_count.clear();
            if vector_files_to_process_frames_count.is_empty() {
                for file in total_frames.clone() {
                    current_frame_count += u64::from(get_frame_count_duration(&file));
                    vector_files_to_process_frames_count.push(current_frame_count);
                }
            }
        }

        let total_frames_count = current_frame_count;

        for file in vector_files_to_process.clone() {
            let dar = get_display_aspect_ratio(&file).to_string();
            current_file_count = current_file_count + 1;
            total_files = vector_files_to_process.len() as i32;
            args.inputpath = file.clone();
            rebuild_temp(true);

            if args.outputpath.is_none() {
                let path = Path::new(&args.inputpath);
                let filename_ext = &args.format;
                let filename_no_ext = path.file_stem().unwrap().to_string_lossy();
                let filename_codec = &args.codec;
                let directory = absolute_path(path.parent().unwrap());
                #[cfg(target_os = "windows")]
                let directory_path = format!("{}{}", directory.trim_end_matches("."), "\\");
                #[cfg(target_os = "linux")]
                let directory_path = format!("{}{}", directory.trim_end_matches("."), "/");
                output_path = format!(
                    "{}{}.{}.{}",
                    directory_path, filename_no_ext, filename_codec, filename_ext
                );
                done_output = format!("{}.{}.{}", filename_no_ext, filename_codec, filename_ext);
                match output_validation_dir(&output_path) {
                    Err(e) => {
                        println!("{:?}", e);
                        exit(1);
                    }
                    Ok(s) => {
                        if s.contains("already exists") {
                            println!("{} already exists, skipping", done_output);
                            continue;
                        }
                    }
                }
            }
            if args.outputpath.is_some() {
                let str_outputpath = &args
                    .outputpath
                    .as_deref()
                    .unwrap_or("default string")
                    .to_owned();
                let path = Path::new(&str_outputpath);
                let filename = path.file_name().unwrap().to_string_lossy();

                output_path = absolute_path(filename.to_string());
                done_output = filename.to_string();
                match output_validation_dir(&output_path) {
                    Err(e) => {
                        println!("{:?}", e);
                        exit(1);
                    }
                    Ok(s) => {
                        if s.contains("already exists") {
                            println!("{} already exists, skipping", done_output);
                            continue;
                        }
                    }
                }
            }

            args.inputpath = absolute_path(file.clone());

            println!(
                "Processing file {} of {} ({}):",
                current_file_count, total_files, done_output
            );
            println!("Input path: {}", args.inputpath);
            println!("Output path: {}", output_path);
            println!("total_frames_count: {}", total_frames_count);
            println!(
                "vector_files_to_process_frames_count: {:?}",
                vector_files_to_process_frames_count
            );
            //exit(1);

            // update status in sqlite database 'reve.db' to processing for this file where filepaths match the current file
            let conn = Connection::open("reve.db").unwrap();
            conn.execute(
                "UPDATE video_info SET status = 'processing' WHERE filepath = ?",
                &[&args.inputpath],
            )
            .unwrap();

            work(
                &args,
                dar.clone(),
                current_file_count as i32,
                total_files,
                done_output.clone(),
                output_path.clone(),
                total_frames_count.clone(),
                vector_files_to_process_frames_count.clone(),
            );

            // Validation
            {
                let in_extension = Path::new(&args.inputpath).extension().unwrap();
                let out_extension = Path::new(&output_path).extension().unwrap();

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
        }
        let elapsed = main_now.elapsed();
        let seconds = elapsed.as_secs() % 60;
        let minutes = (elapsed.as_secs() / 60) % 60;
        let hours = (elapsed.as_secs() / 60) / 60;
        println!(
            "done {} files in {}h:{}m:{}s",
            count, hours, minutes, seconds
        );
    }

    #[cfg(target_os = "windows")]
    let folder_args = "\\";
    #[cfg(target_os = "linux")]
    let folder_args = "/";

    if md.is_file() {
        let dar = get_display_aspect_ratio(&args.inputpath).to_string();
        current_file_count = 1;
        let mut total_frames_count = u64::from(get_frame_count(&args.inputpath));
        if total_frames_count == 0 {
            total_frames_count = u64::from(get_frame_count_tag(&args.inputpath));
        }
        let directory = Path::new(&args.inputpath)
            .parent()
            .unwrap()
            .to_str()
            .unwrap();
        if args.outputpath.is_none() {
            let path = Path::new(&args.inputpath);
            let filename_ext = &args.format;
            let filename_no_ext = path.file_stem().unwrap().to_string_lossy();
            let filename_codec = &args.codec;
            output_path = format!(
                "{}{}{}.{}.{}",
                directory, folder_args, filename_no_ext, filename_codec, filename_ext
            );
            done_output = format!("{}.{}.{}", filename_no_ext, filename_codec, filename_ext);
        }
        if args.outputpath.is_some() {
            let str_outputpath = &args
                .outputpath
                .as_deref()
                .unwrap_or("default string")
                .to_owned();
            let path = Path::new(&str_outputpath);
            let filename = path.file_name().unwrap().to_string_lossy();
            output_path = absolute_path(filename.to_string());
            done_output = filename.to_string();
        }
        match output_validation(&output_path) {
            Err(e) => println!("{:?}", e),
            _ => (),
        }
        clear().expect("failed to clear screen");
        total_files = 1;

        let temp_vector = vec![total_frames_count];

        let ffprobe_output = Command::new("ffprobe")
            .args([
                "-i",
                args.inputpath.as_str(),
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
            .unwrap();
        //.\ffprobe.exe -i '\\192.168.1.99\Data\Animes\Agent AIKa\Saison 2\Agent AIKa - S02E03 - Trial 3 Deep Blue Girl.mkv' -v error -select_streams v -show_entries stream -show_format -show_data_hash sha256 -show_streams -of json
        let json_output = std::str::from_utf8(&ffprobe_output.stdout[..]).unwrap();
        let height = check_ffprobe_output_i8(json_output, &resolution.to_string());
        if height.unwrap() == 1 {
            work(
                &args,
                dar,
                current_file_count as i32,
                total_files,
                done_output,
                output_path.clone(),
                total_frames_count,
                temp_vector,
            );
        } else {
            println!(
                "{} is bigger than {}p",
                args.inputpath,
                resolution.to_string()
            );
            println!("Set argument -r to a higher value");
            exit(1);
        }

        // Validation
        {
            let in_extension = Path::new(&args.inputpath).extension().unwrap();
            let out_extension = Path::new(&output_path).extension().unwrap();

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
    }
}

fn work(
    args: &Args,
    dar: String,
    current_file_count: i32,
    total_files: i32,
    done_output: String,
    output_path: String,
    total_frames_count: u64,
    vector_files_to_process_frames_count: Vec<u64>,
) {
    let work_now = Instant::now();

    #[cfg(target_os = "windows")]
    let args_path = Path::new("temp\\args.temp");
    #[cfg(target_os = "windows")]
    let video_parts_path = "temp\\video_parts\\";
    #[cfg(target_os = "windows")]
    let temp_video_path = format!("temp\\temp.{}", &args.format);
    #[cfg(target_os = "windows")]
    let txt_list_path = "temp\\parts.txt";

    #[cfg(target_os = "linux")]
    let args_path = Path::new("/dev/shm/args.temp");
    #[cfg(target_os = "linux")]
    let video_parts_path = "/dev/shm/video_parts/";
    #[cfg(target_os = "linux")]
    let temp_video_path = format!("/dev/shm/temp.{}", &args.format);
    #[cfg(target_os = "linux")]
    let txt_list_path = "/dev/shm/parts.txt";

    if Path::new(&args_path).exists() {
        //Check if previous file is used, if yes, continue upscale without asking
        let old_args_json = fs::read_to_string(&args_path).expect("Unable to read file");
        let old_args: Args = serde_json::from_str(&old_args_json).unwrap();
        let previous_file = Path::new(&old_args.inputpath);
        let md = fs::metadata(&args.inputpath).unwrap();

        // Check if same file is used as previous upscale and if yes, resume
        if args.inputpath == previous_file.to_string_lossy() {
            if md.is_file() {
                println!(
                    "Same file! '{}' Resuming...",
                    previous_file.file_name().unwrap().to_str().unwrap()
                );
                // Resume upscale
                rebuild_temp(true);
                clear().expect("failed to clear screen");
                println!("{}", "resuming upscale".to_string().green());
            }
        } else {
            // Remove and start new
            rebuild_temp(false);
            match fs::remove_file(&txt_list_path) {
                Ok(()) => "ok",
                Err(_e) if _e.kind() == ErrorKind::NotFound => "not found",
                Err(_e) => "other",
            };
            match fs::remove_file(&temp_video_path) {
                Ok(()) => "ok",
                Err(_e) if _e.kind() == ErrorKind::NotFound => "not found",
                Err(_e) => "other",
            };

            let serialized_args = serde_json::to_string(&args).unwrap();

            let md = metadata(&args.inputpath).unwrap();
            if md.is_file() {
                fs::write(&args_path, serialized_args).expect("Unable to write file");
            }
            clear().expect("failed to clear screen");
            println!(
                "{}",
                "deleted all temporary files, parsing console input"
                    .to_string()
                    .green()
            );
        }
    } else {
        // Remove and start new
        rebuild_temp(false);
        match fs::remove_file(&txt_list_path) {
            Ok(()) => "ok",
            Err(_e) if _e.kind() == ErrorKind::NotFound => "not found",
            Err(_e) => "other",
        };
        match fs::remove_file(&temp_video_path) {
            Ok(()) => "ok",
            Err(_e) if _e.kind() == ErrorKind::NotFound => "not found",
            Err(_e) => "other",
        };

        let serialized_args = serde_json::to_string(&args).unwrap();

        let md = metadata(&args.inputpath).unwrap();
        if md.is_file() {
            fs::write(&args_path, serialized_args).expect("Unable to write file");
        }
        clear().expect("failed to clear screen");
        println!(
            "{}",
            "deleted all temporary files, parsing console input"
                .to_string()
                .green()
        );
    }

    // check if files are marked as processing in database that is not the current file and set status as 'pending'
    let conn = Connection::open("reve.db").unwrap();
    conn.execute(
        "UPDATE video_info SET status = 'pending' WHERE status = 'processing' AND filepath != ?1",
        params![args.inputpath],
    )
    .unwrap();

    let mut frame_position;
    let filename = Path::new(&args.inputpath)
        .file_name()
        .unwrap()
        .to_str()
        .unwrap();

    let mut total_frame_count = get_frame_count(&args.inputpath);

    if total_frame_count == 0 {
        total_frame_count = get_frame_count_tag(&args.inputpath);
    }

    if total_frame_count == 0 {
        total_frame_count = get_frame_count_duration(&args.inputpath);
    }

    let original_frame_rate = get_frame_rate(&args.inputpath);

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
            "{}/{}, {}, total segments: {}, last segment size: {}, codec: {} (ctrl+c to exit)",
            current_file_count,
            total_files,
            filename.green(),
            parts_num,
            last_part_size,
            _codec
        )
        .yellow()
    );

    {
        let mut unprocessed_indexes = Vec::new();
        for i in 0..parts_num {
            #[cfg(target_os = "linux")]
            let n = format!("{}/{}.{}", video_parts_path, i, &args.format);
            #[cfg(target_os = "windows")]
            let n = format!("{}\\{}.{}", video_parts_path, i, &args.format);
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
                let mut c = get_frame_count(&p.display().to_string());
                if c == 0 {
                    c = get_frame_count_tag(&p.display().to_string());
                }
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

        let count;
        if current_file_count == 1 {
            count = total_frames_count;
        } else {
            count = total_frames_count
                - vector_files_to_process_frames_count[(current_file_count - 2) as usize];
        }
        frame_position = (total_frames_count - count as u64)
            + (parts_num as usize - unprocessed_indexes.len()) as u64 * args.segmentsize as u64;

        let mut export_handle = thread::spawn(move || {});
        let mut merge_handle = thread::spawn(move || {});
        let total_frames_style = "[fram][{elapsed_precise}] [{wide_bar:.green/white}] {pos:>7}/{len:7} total frames             eta: {eta:<7}";
        let info_style = "[info][{elapsed_precise}] [{wide_bar:.green/white}] {pos:>7}/{len:7} processed segments       eta: {eta:<7}";
        let expo_style = "[expo][{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos:>7}/{len:7} exporting segment        {per_sec:<12}";
        let upsc_style = "[upsc][{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos:>7}/{len:7} upscaling segment        {per_sec:<12}";
        let merg_style = "[merg][{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos:>7}/{len:7} merging segment          {per_sec:<12}";
        let _alt_style = "[]{elapsed}] {wide_bar:.cyan/blue} {spinner} {percent}% {human_len:>7}/{human_len:7} {per_sec} {eta}";

        let m = MultiProgress::new();
        let pb = m.add(ProgressBar::new(parts_num as u64));
        pb.set_style(
            ProgressStyle::default_bar()
                .template(info_style)
                .unwrap()
                .progress_chars("#>-"),
        );
        let mut last_pb = pb.clone();

        //let progress_bar = m.insert_after(&last_pb, ProgressBar::new(total_files as u64));
        let progress_bar_frames =
            m.insert_after(&last_pb, ProgressBar::new(total_frames_count as u64));
        progress_bar_frames.set_style(
            ProgressStyle::default_bar()
                .template(total_frames_style)
                .unwrap()
                .progress_chars("#>-"),
        );
        progress_bar_frames.set_position(frame_position as u64);

        last_pb = progress_bar_frames.clone();

        // Initial export
        if !unprocessed_indexes.is_empty() {
            let index = unprocessed_indexes[0].index;
            let _inpt = &args.inputpath.clone();
            #[cfg(target_os = "linux")]
            let _outpt = format!("/dev/shm/tmp_frames/{}/frame%08d.png", index);
            #[cfg(target_os = "windows")]
            let _outpt = format!("temp\\tmp_frames\\{}\\frame%08d.png", index);
            let _start_time = if index == 0 {
                String::from("0")
            } else {
                ((index * args.segmentsize - 1) as f32
                    / original_frame_rate.parse::<f32>().unwrap())
                .to_string()
            };
            #[cfg(target_os = "linux")]
            let _index_dir = format!("/dev/shm/tmp_frames/{}", index);
            #[cfg(target_os = "windows")]
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

            // TODO LINUX: /dev/shm to export the frames
            // https://github.com/PauMAVA/cargo-ramdisk
            // Windows doesn't really have something native like a ramdisk sadly
            export_frames(
                &args.inputpath,
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
                let _inpt = args.inputpath.clone();
                #[cfg(target_os = "linux")]
                let _outpt = format!("/dev/shm/tmp_frames/{}/frame%08d.png", index);
                #[cfg(target_os = "windows")]
                let _outpt = format!("temp\\tmp_frames\\{}\\frame%08d.png", index);
                let _start_time = ((index * args.segmentsize - 1) as f32
                    / original_frame_rate.parse::<f32>().unwrap())
                .to_string();
                #[cfg(target_os = "linux")]
                let _index_dir = format!("/dev/shm/tmp_frames/{}", index);
                #[cfg(target_os = "windows")]
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

            #[cfg(target_os = "linux")]
            let inpt_dir = format!("/dev/shm/tmp_frames/{}", segment.index);
            #[cfg(target_os = "linux")]
            let outpt_dir = format!("/dev/shm/out_frames/{}", segment.index);
            #[cfg(target_os = "windows")]
            let inpt_dir = format!("temp\\tmp_frames\\{}", segment.index);
            #[cfg(target_os = "windows")]
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

            frame_position = upscale_frames(
                &inpt_dir,
                &outpt_dir,
                &args.scale.to_string(),
                progress_bar,
                progress_bar_frames.clone(),
                frame_position,
            )
            .expect("could not upscale frames");

            merge_handle.join().unwrap();

            let _codec = args.codec.clone();
            #[cfg(target_os = "linux")]
            let _inpt = format!("/dev/shm/out_frames/{}/frame%08d.png", segment.index);
            #[cfg(target_os = "linux")]
            let _outpt = format!("/dev/shm/video_parts/{}.{}", segment.index, &args.format);
            #[cfg(target_os = "windows")]
            let _inpt = format!("temp\\out_frames\\{}\\frame%08d.png", segment.index);
            #[cfg(target_os = "windows")]
            let _outpt = format!("temp\\video_parts\\{}.{}", segment.index, &args.format);
            let _frmrt = original_frame_rate.clone();
            let _crf = args.crf.clone().to_string();
            let _preset = args.preset.clone();
            let _x265_params = args.x265params.clone();
            let _extension = args.format.clone();

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
                    merge_frames_svt_hevc(&_inpt, &_outpt, &_codec, &_frmrt, &_crf, progress_bar)
                        .unwrap();
                    fs::remove_dir_all(&outpt_dir).unwrap();
                } else if &_codec == "libsvtav1" {
                    merge_frames_svt_av1(&_inpt, &_outpt, &_codec, &_frmrt, &_crf, progress_bar)
                        .unwrap();
                    fs::remove_dir_all(&outpt_dir).unwrap();
                } else if &_codec == "libx265" {
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
    let choosen_extension = &args.format;
    #[cfg(target_os = "linux")]
    let mut f_content = format!("file 'video_parts/0.{}'", choosen_extension);
    #[cfg(target_os = "windows")]
    let mut f_content = format!("file 'video_parts\\0.{}'", choosen_extension);

    for part_number in 1..parts_num {
        #[cfg(target_os = "linux")]
        let video_part_path = format!("video_parts/{}.{}", part_number, choosen_extension);
        #[cfg(target_os = "windows")]
        let video_part_path = format!("video_parts\\{}.{}", part_number, choosen_extension);
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
                if dar == "0" {
                    merge_video_parts(&txt_list_path.to_string(), &temp_video_path.to_string());
                } else {
                    merge_video_parts_dar(
                        &txt_list_path.to_string(),
                        &temp_video_path.to_string(),
                        &dar,
                    );
                }
                count += 1;
            }
        }
    }

    //Check if there is invalid bin data in the input file
    let bin_data = get_bin_data(&args.inputpath);
    if bin_data != "" {
        println!("invalid data at index: {}, skipping this one", bin_data);
        println!("copying streams");
        copy_streams_no_bin_data(&temp_video_path.to_string(), &args.inputpath, &output_path);
    } else {
        println!("copying streams");
        copy_streams(&temp_video_path.to_string(), &args.inputpath, &output_path);
    }

    //Check if file has been copied successfully to output path, if so, update database
    let p = Path::new(&output_path);
    if p.exists() {
        if fs::File::open(p).unwrap().metadata().unwrap().len() == 0 {
            panic!("failed to copy streams");
        }
        // update sqlite database "reve.db" entry with status "done" using update_db_status function;
        let conn = Connection::open("reve.db").unwrap();
        let db_status = update_db_status(&conn, &args.inputpath, "done");
        match db_status {
            Ok(_) => println!("updated database"),
            Err(e) => println!("failed to update database: {}", e),
        }
    } else {
        panic!("failed to copy streams");
    }

    clear().expect("failed to clear screen");
    let elapsed = work_now.elapsed();
    let seconds = elapsed.as_secs() % 60;
    let minutes = (elapsed.as_secs() / 60) % 60;
    let hours = (elapsed.as_secs() / 60) / 60;

    let ancestors = Path::new(&args.inputpath).file_name().unwrap();
    println!(
        "done {:?} to {:?} in {}h:{}m:{}s",
        ancestors, done_output, hours, minutes, seconds
    );
}
