mod cli;

use clap::Parser;
use std::process::ExitCode;

use lazypock_core::Result;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("lazypock: {err}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<()> {
    print!("serving");
    Ok(())
}
