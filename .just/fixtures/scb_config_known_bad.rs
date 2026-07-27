use crate::config;

fn load_workspace_config(team_dir: &std::path::Path) {
    let _ = config::load_team_config(team_dir);
}

fn send_bad(team_dir: &std::path::Path) {
    let _ = load_team_config(team_dir);
}
