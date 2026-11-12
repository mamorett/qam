use crate::config::{self, Camera};
use crate::errors::QamError;
use crate::timefmt;

#[allow(clippy::too_many_arguments)]
pub fn execute(
    name: String,
    host: String,
    port: u16,
    protocol: String,
    user: String,
    pass: String,
    path: String,
    rtsp_transport: String,
    stream: String,
    rtsp_client: String,
    no_audio: bool,
    audio_codec: String,
    config_path: Option<&str>,
) -> Result<(), QamError> {
    if name.trim().is_empty() || host.trim().is_empty() {
        return Err(QamError::Usage("name and host are required".to_string()));
    }
    let port = if port == 0 { 554 } else { port };
    let protocol = if protocol.trim().is_empty() {
        "rtsp".to_string()
    } else {
        protocol
    };

    let target_path = config::resolve_load_path(config_path.unwrap_or(""))?;
    let mut cfg = config::load(config_path.unwrap_or(""))?;

    let cam = Camera {
        name,
        host,
        port,
        scheme: protocol,
        user,
        pass,
        path,
        rtsp_transport,
        stream,
        rtsp_client,
        no_audio,
        audio_codec,
        ..Default::default()
    };

    let cam_name = cam.name.clone();
    let created = cfg.upsert_camera(cam);
    config::save_to_path(&target_path, &cfg)?;

    if created {
        println!("Added camera {}", timefmt::go_quote(&cam_name));
    } else {
        println!("Updated camera {}", timefmt::go_quote(&cam_name));
    }

    Ok(())
}
