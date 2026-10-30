use std::process::ExitCode;

use clap::Parser;
use poolproof::cli::{Cli, run};

#[tokio::main]
async fn main() -> ExitCode {
    // Exit quietly when the reader goes away (`poolproof list | head`) instead of
    // panicking on the failed write, like other command-line tools.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    match run(Cli::parse()).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        }
    }
}
