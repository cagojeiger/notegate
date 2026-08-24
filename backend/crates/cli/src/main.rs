use std::io;
use std::process::ExitCode;

use clap::Parser as _;
use notegate_cli::{Cli, execute};
use serde_json::Value;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match execute(cli).await {
        Ok(value) => match write_json(io::stdout().lock(), &value) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("notegate-cli: could not write stdout: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            if let Err(write_error) = write_json(io::stderr().lock(), error.body()) {
                eprintln!("notegate-cli: could not write stderr: {write_error}");
                return ExitCode::FAILURE;
            }
            ExitCode::from(error.exit_code())
        }
    }
}

fn write_json(mut writer: impl io::Write, value: &Value) -> io::Result<()> {
    serde_json::to_writer(&mut writer, value).map_err(io::Error::other)?;
    writer.write_all(b"\n")
}
