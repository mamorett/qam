pub mod cli;
pub mod config;
pub mod errors;
pub mod rtsp_url;
pub mod timefmt;
pub mod version;

/// Runs the CLI with the given command-line arguments.
pub async fn run<I, T>(args: I) -> Result<(), errors::QamError>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    cli::root::run_cli(args).await
}
