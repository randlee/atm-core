use std::env;
use std::fs;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use serde_json::json;

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
    let extra_args: Vec<String> = args.collect();

    let payload = env::var("ATM_POST_SEND").unwrap_or_default();

    match mode.as_str() {
        "capture" => {
            let _ = fs::write(&output_path, payload);
            ExitCode::SUCCESS
        }
        "capture-meta" => {
            let parsed_payload = serde_json::from_str::<serde_json::Value>(&payload)
                .unwrap_or_else(|_| json!(payload));
            let _ = fs::write(
                &output_path,
                serde_json::to_vec(&json!({
                    "payload": parsed_payload,
                    "args": extra_args,
                }))
                .unwrap_or_default(),
            );
            ExitCode::SUCCESS
        }
        "fail" => ExitCode::from(3),
        "sleep" => {
            let _ = fs::write(&output_path, payload);
            thread::sleep(Duration::from_secs(6));
            ExitCode::SUCCESS
        }
        _ => ExitCode::from(2),
    }
}
