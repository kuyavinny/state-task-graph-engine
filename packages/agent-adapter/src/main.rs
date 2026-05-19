mod cli;
mod config;
mod error;
mod graph_client;
mod graph_runner;
mod graph_types;
mod response;

use clap::Parser;
use cli::Cli;

fn main() {
    let args = Cli::parse();
    if let Err(ref err) = args.run() {
        let _ = response::output_failure("", "unknown", err);
        std::process::exit(1);
    }
}
