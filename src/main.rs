use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    qam::signals::init_signals();

    if std::env::var_os("RUST_LOG").is_some() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .with_writer(std::io::stderr)
            .try_init();
    }

    match qam::run(std::env::args_os()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            if !matches!(err, qam::errors::QamError::Interrupted(_)) {
                eprintln!("{err}");
            }
            ExitCode::from(err.exit_code())
        }
    }
}
