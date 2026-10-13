//! Command-line interface definition.

use clap::{Parser, Subcommand};

/// lazypock — a PocketBase-style backend in Rust + Postgres.
#[derive(Debug, Parser)]
#[command(name = "lazypock", version, about, long_about = None)]
pub struct Cli {
    /// Command to run. Defaults to `serve`.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Top-level commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start the HTTP API server (default when no command is given).
    Serve,
    /// Apply pending database migrations.
    Migrate,
    /// Manage superuser accounts.
    Superuser {
        /// Superuser subcommand.
        #[command(subcommand)]
        command: SuperuserCommand,
    },
    /// Print the resolved configuration (credentials redacted) and exit.
    Config,
    /// Exit 0 when the server reports healthy.
    Healthcheck,
}

/// `lazypock superuser <subcommand>`.
#[derive(Debug, Subcommand)]
pub enum SuperuserCommand {
    /// Create a superuser.
    Create {
        /// Superuser email address.
        #[arg(long)]
        email: String,
        /// Password. Prefer the `LAZYPOCK_SUPERUSER_PASSWORD` environment
        /// variable so the secret does not land in shell history.
        #[arg(long, env = "LAZYPOCK_SUPERUSER_PASSWORD", hide_env_values = true)]
        password: String,
    },
}
