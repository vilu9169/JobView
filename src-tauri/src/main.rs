#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if mailview_lib::run().is_err() {
        eprintln!(
            "JobView could not start. Check the local application logs and Windows prerequisites."
        );
        std::process::exit(1);
    }
}
