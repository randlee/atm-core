use sc_observability_log::{Level, event, instrument};

#[instrument(name = "ui.supported", skip_all, fields(answer = 42))]
fn supported() -> u32 { 42 }

fn main() {
    event!(name: "supported", target: "ui", Level::INFO, answer = 42);
    let _ = supported();
}
