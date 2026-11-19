use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "lazypock", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Healthcheck,
}
