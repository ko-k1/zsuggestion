//! Thin CLI entry point. Parses arguments and dispatches to [`cli::run`].

use clap::Parser;
use zsuggestion::cli;

fn main() {
    if let Err(error) = cli::run(cli::Cli::parse()) {
        eprintln!("zsuggestion: {error:#}");
        std::process::exit(1);
    }
}
