use std::fs;
use std::io::ErrorKind;
use std::process::Command;

#[test]
fn run_verify() {
    match fs::remove_file("target\\debug\\temp\\parts.txt") {
        Ok(()) => "ok",
        Err(_e) if _e.kind() == ErrorKind::NotFound => "not found",
        Err(_e) => "other",
    };
    match fs::remove_file("target\\debug\\temp\\temp.mp4") {
        Ok(()) => "ok",
        Err(_e) if _e.kind() == ErrorKind::NotFound => "not found",
        Err(_e) => "other",
    };
    match fs::remove_file("target\\debug\\temp\\args.temp") {
        Ok(()) => "ok",
        Err(_e) if _e.kind() == ErrorKind::NotFound => "not found",
        Err(_e) => "other",
    };
    match fs::remove_file("out.mp4") {
        Ok(()) => "ok",
        Err(_e) if _e.kind() == ErrorKind::NotFound => "not found",
        Err(_e) => "other",
    };
    Command::new("target\\debug\\reve")
        .args(["-i", "assets\\test.mp4", "-s", "2", "out.mp4"])
        .output()
        .unwrap();
    match fs::remove_file("out.mp4") {
        Ok(()) => "ok",
        _ => panic!("run failed"),
    };
}
