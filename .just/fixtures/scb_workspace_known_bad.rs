use crate::config::load_config;

fn run_bad(current_dir: &std::path::Path) {
    let _ = load_config(current_dir);
}
