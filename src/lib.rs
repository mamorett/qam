pub mod version;

/// Runs the CLI with the given command-line arguments.
pub async fn run<I, T>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let collected: Vec<String> = args
        .into_iter()
        .map(|a| a.into().to_string_lossy().into_owned())
        .collect();
    if collected
        .iter()
        .skip(1)
        .any(|a| a == "--version" || a == "-v" || a == "version")
    {
        println!("{}", version::VERSION);
        return Ok(());
    }
    Ok(())
}
