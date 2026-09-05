//! The `pm-debug` binary — a visibly distinct development build that runs beside
//! the real `pm` (PM-88). All wiring lives in the `pm` library crate.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    pm::run(pm::DEBUG);
}
