use std::time::Duration;

use crate::cli::style::Styler;
use crate::config::{self, Config};
use crate::discovery::{self, Device};
use crate::errors::QamError;

pub async fn execute(
    timeout: Duration,
    include_info: bool,
    config_path: Option<&str>,
) -> Result<(), QamError> {
    let sty = Styler::new();

    // Load config ignoring errors (Go parity)
    let cfg = config::load(config_path.unwrap_or("")).unwrap_or_default();

    let devs = discovery::discover(timeout, None).await?;
    if devs.is_empty() {
        println!(
            "{}",
            sty.warn("No devices found. Ensure cameras and this host are on the same LAN.")
        );
        return Ok(());
    }

    let client = reqwest::Client::new();
    for d in devs {
        let mut info_str = String::new();
        if include_info {
            let info = fetch_info(&client, &cfg, &d).await;
            if !info.is_empty() {
                info_str = format!(" [{info}]");
            }
        }

        println!(
            "{}\t(add: qam add --name cam-{} --host {} --user <user> --pass <pass>){}",
            sty.ok(&d.host),
            safe_name(&d.host),
            d.host,
            info_str
        );
    }

    Ok(())
}

fn safe_name(host: &str) -> &str {
    if let Some(idx) = host.find(':') {
        &host[..idx]
    } else {
        host
    }
}

async fn fetch_info(client: &reqwest::Client, cfg: &Config, d: &Device) -> String {
    let (user, pass) = find_creds(cfg, &d.host);
    let info = match discovery::info::fetch_device_info(client, &d.address, &user, &pass).await {
        Ok(i) => i,
        Err(_) => return String::new(),
    };

    let mut parts = Vec::new();
    if !info.model.is_empty() {
        parts.push(info.model);
    }
    if !info.firmware.is_empty() {
        parts.push(format!("fw {}", info.firmware));
    }
    if !info.manufacturer.is_empty() {
        parts.push(info.manufacturer);
    }

    parts.join(", ")
}

fn find_creds(cfg: &Config, host: &str) -> (String, String) {
    for cam in &cfg.cameras {
        if cam.host == host || host.starts_with(&format!("{}:", cam.host)) {
            return (cam.user.clone(), cam.pass.clone());
        }
    }
    (String::new(), String::new())
}
