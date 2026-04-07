use std::env;
use std::fs;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let mode = match args.next() {
        Some(mode) => mode,
        None => return ExitCode::from(2),
    };
    let output_path = match args.next() {
        Some(path) => path,
        None => return ExitCode::from(2),
    };

    let payload = env::var("ATM_POST_SEND").unwrap_or_default();
    let _ = fs::write(&output_path, payload);

    match mode.as_str() {
        "capture" => ExitCode::SUCCESS,
        "fail" => ExitCode::from(3),
        "sleep" => {
            thread::sleep(Duration::from_secs(6));
            ExitCode::SUCCESS
        }
        _ => ExitCode::from(2),
    }
}
