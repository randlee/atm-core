//! Portable deterministic stand-in for the published Herdr CLI.

use std::io::Write;

fn main() {
    let mode = std::env::var("FAKE_HERDR_MODE").unwrap_or_else(|_| "exit-success".to_owned());
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match mode.as_str() {
        "exit-success" => stdout(r#"{"result":{"agent":{"name":"fake","agent_status":"idle"}}}"#),
        "exit-failure" => failure("internal_error"),
        mode if mode.starts_with("stderr-envelope:") => {
            failure(mode.trim_start_matches("stderr-envelope:"));
        }
        "stdout-json-line" => stdout_crlf(
            r#"{"result":{"agent":{"name":"fake","agent_status":"working","unknown":"ignored"}}}"#,
        ),
        "sleep-past-deadline" => {
            std::thread::park();
            stdout(r#"{"result":{}}"#);
        }
        "echo-argv-and-herdr-session" => {
            let session = std::env::var("HERDR_SESSION").ok();
            let agent = args.get(2).map(String::as_str).unwrap_or_default();
            let session_name = session.as_deref().unwrap_or_default();
            let rendered_args = args
                .iter()
                .map(|argument| format!("\"{}\"", escape(argument)))
                .collect::<Vec<_>>()
                .join(",");
            let session = session
                .as_deref()
                .map(|value| format!("\"{}\"", escape(value)))
                .unwrap_or_else(|| "null".to_owned());
            stdout(&format!(
                "{{\"result\":{{\"agent\":{{\"name\":\"{}:{}\",\"agent_status\":\"idle\"}},\"argv\":[{rendered_args}],\"herdr_session\":{session}}}}}",
                escape(agent),
                escape(session_name)
            ));
        }
        mode if mode.starts_with("status-server-json:") => {
            let version = mode.trim_start_matches("status-server-json:");
            stdout(&format!(
                "{{\"result\":{{\"version\":\"{}\",\"protocol\":20,\"capabilities\":[]}}}}",
                escape(version)
            ));
        }
        mode if mode.starts_with("replay:") => replay(mode.trim_start_matches("replay:")),
        "v0.8.0-blocked-prompt" => {
            if std::env::var("FAKE_HERDR_DELTA").as_deref() == Ok("blocked-prompt") {
                failure("agent_prompt_stalled");
            }
            failure("protocol_mismatch");
        }
        _ => failure("protocol_mismatch"),
    }
}

fn replay(directory: &str) {
    let path = std::path::Path::new(directory).join("response.json");
    match std::fs::read_to_string(path) {
        Ok(response) => stdout(&response),
        Err(_) => failure("protocol_mismatch"),
    }
}

fn stdout(value: &str) {
    let _ = std::io::stdout().write_all(value.as_bytes());
    let _ = std::io::stdout().write_all(b"\n");
}

fn stdout_crlf(value: &str) {
    let _ = std::io::stdout().write_all(value.as_bytes());
    let _ = std::io::stdout().write_all(b"\r\n");
}

fn failure(code: &str) -> ! {
    let _ = std::io::stderr().write_all(
        format!(
            "{{\"error\":{{\"code\":\"{}\",\"message\":\"fake\"}}}}\n",
            escape(code)
        )
        .as_bytes(),
    );
    std::process::exit(1);
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}
