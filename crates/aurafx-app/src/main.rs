#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> aurafx_app::eframe::Result<()> {
    aurafx_app::main_desktop()
}
