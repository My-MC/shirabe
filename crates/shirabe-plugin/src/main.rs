//! Standalone application entry point.
//!
//! Run `shirabe --help` for audio backend and device options.

// Release builds on Windows should not open a console window next to the GUI.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use nih_plug::prelude::*;
use shirabe_plugin::Shirabe;

fn main() {
    nih_export_standalone::<Shirabe>();
}
