use crate::cli::style::Styler;
use crate::config;
use crate::errors::QamError;

pub fn execute(config_path: Option<&str>, no_color: bool) -> Result<(), QamError> {
    let sty = Styler::with_color(!no_color && Styler::is_color_enabled());
    let cfg = config::load(config_path.unwrap_or(""))?;

    if cfg.cameras.is_empty() {
        let msg = sty.warn(
            "No cameras saved. Add one with: qam add --name cam1 --host 192.168.1.50 --user tapo --pass secret",
        );
        println!("{msg}");
        return Ok(());
    }

    let mut sorted_cams = cfg.cameras.clone();
    sorted_cams.sort_by(|a, b| a.name.cmp(&b.name));

    for cam in sorted_cams {
        let auth = if !cam.user.is_empty() && !cam.pass.is_empty() {
            format!("{}:***", cam.user)
        } else {
            cam.user.clone()
        };
        let proto = cam.scheme.to_ascii_lowercase();
        println!(
            "{:<12} host={} port={} proto={} auth={}",
            cam.name, cam.host, cam.port, proto, auth
        );
    }

    Ok(())
}
