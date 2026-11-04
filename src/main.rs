mod model;
mod notify;
mod schedule;
mod store;
mod tui;

use anyhow::Result;
use chrono::{Local, Timelike};
use clap::{Parser, Subcommand};

use store::{SqliteStore, Store};

/// Minimal terminal timeblocking: plan your day as stops on a route.
///
/// Run without arguments to open the day planner.
/// Data lives in ~/.local/share/bus/bus.db (override with $BUS_DB).
#[derive(Parser)]
#[command(name = "bus", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Print the current and next stop on one line (for tmux or a status bar).
    Now {
        /// Template with {title}, {left}, {next}, {next_start}, {next_in}.
        #[arg(long)]
        format: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let store = SqliteStore::open(&SqliteStore::default_path()?)?;
    match cli.command {
        None => tui::run(Box::new(store)),
        Some(Command::Now { format }) => {
            let now = Local::now().naive_local();
            let day = store.load_day(now.date())?;
            let minute = (now.hour() * 60 + now.minute()) as i32;
            println!("{}", schedule::status_line(&day, minute, format.as_deref()));
            Ok(())
        }
    }
}
