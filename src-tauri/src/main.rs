// Windows: no console window behind the application in a release build.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    comfyvault_lib::run()
}
