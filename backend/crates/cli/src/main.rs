use std::io;
use std::process::ExitCode;

use clap::Parser as _;
use clap::error::ErrorKind;
use notegate_cli::{Cli, CliError, execute};
use serde_json::Value;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            return print_clap_output(error);
        }
        Err(error) => {
            return write_cli_error(&CliError::invalid_input(
                "invalid_arguments",
                error.to_string(),
            ));
        }
    };
    match execute(cli).await {
        Ok(value) => match write_json(io::stdout().lock(), &value) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("notegate-cli: could not write stdout: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => write_cli_error(&error),
    }
}

fn print_clap_output(error: clap::Error) -> ExitCode {
    match error.print() {
        Ok(()) => ExitCode::SUCCESS,
        Err(write_error) if write_error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(write_error) => {
            eprintln!("notegate-cli: could not write help: {write_error}");
            ExitCode::FAILURE
        }
    }
}

fn write_cli_error(error: &CliError) -> ExitCode {
    if let Err(write_error) = write_json(io::stderr().lock(), error.body()) {
        eprintln!("notegate-cli: could not write stderr: {write_error}");
        return ExitCode::FAILURE;
    }
    ExitCode::from(error.exit_code())
}

fn write_json(mut writer: impl io::Write, value: &Value) -> io::Result<()> {
    serde_json::to_writer(&mut writer, value).map_err(io::Error::other)?;
    writer.write_all(b"\n")
}
