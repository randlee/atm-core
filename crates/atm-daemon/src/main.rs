fn main() {
    let exit_code = match atm_daemon::run_foreground() {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    };
    std::process::exit(exit_code);
}
