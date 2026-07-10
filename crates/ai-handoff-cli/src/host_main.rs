#![cfg_attr(windows, windows_subsystem = "windows")]

use clap::Parser;

fn main() {
    let args = ai_handoff_cli::host_runtime::HostArgs::parse();
    let code = ai_handoff_cli::host_runtime::run(args).unwrap_or(1);
    std::process::exit(code);
}
