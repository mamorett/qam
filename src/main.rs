use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    match qam::run(std::env::args_os()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Error: {err}");
            ExitCode::from(1)
        }
    }
}
