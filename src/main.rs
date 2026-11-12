use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    match qam::run(std::env::args_os()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(err.exit_code())
        }
    }
}
