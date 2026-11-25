#![windows_subsystem = "windows"]

mod control;
mod ui;

fn main() -> std::process::ExitCode {
    match ui::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(message) => {
            ui::fatal(&message);
            std::process::ExitCode::FAILURE
        }
    }
}
