use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::errors::QamError;

pub const DEFAULT_PORT: u16 = 554;
pub const DEFAULT_SCHEME: &str = "rtsp";

fn is_zero_u16(v: &u16) -> bool {
    *v == 0
}

fn is_zero_f64(v: &f64) -> bool {
    *v == 0.0
}

fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Defaults {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rtsp_transport: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rtsp_auth: String,
    #[serde(
        default,
        with = "crate::timefmt::serde_duration",
        skip_serializing_if = "crate::timefmt::is_zero"
    )]
    pub timeout: Duration,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub threshold: f64,
    #[serde(
        default,
        with = "crate::timefmt::serde_duration",
        skip_serializing_if = "crate::timefmt::is_zero"
    )]
    pub clip_duration: Duration,
    #[serde(
        default,
        with = "crate::timefmt::serde_duration",
        skip_serializing_if = "crate::timefmt::is_zero"
    )]
    pub cooldown: Duration,
}

impl Defaults {
    pub fn is_empty(&self) -> bool {
        self.rtsp_transport.is_empty()
            && self.rtsp_auth.is_empty()
            && self.timeout.is_zero()
            && self.threshold == 0.0
            && self.clip_duration.is_zero()
            && self.cooldown.is_zero()
    }

    pub fn normalize(&mut self) {
        self.rtsp_transport = self.rtsp_transport.trim().to_string();
        self.rtsp_auth = self.rtsp_auth.trim().to_ascii_lowercase();
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SlackConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub token: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub default_channel: String,
}

impl SlackConfig {
    pub fn is_empty(&self) -> bool {
        self.token.is_empty() && self.default_channel.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub host: String,
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub port: u16,
    #[serde(default, alias = "username", skip_serializing_if = "String::is_empty")]
    pub user: String,
    #[serde(default, alias = "password", skip_serializing_if = "String::is_empty")]
    pub pass: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
    #[serde(default, alias = "protocol", skip_serializing_if = "String::is_empty")]
    pub scheme: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rtsp_transport: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rtsp_auth: String,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub threshold: f64,
    #[serde(
        default,
        with = "crate::timefmt::serde_duration",
        skip_serializing_if = "crate::timefmt::is_zero"
    )]
    pub clip_duration: Duration,
    #[serde(
        default,
        with = "crate::timefmt::serde_duration",
        skip_serializing_if = "crate::timefmt::is_zero"
    )]
    pub cooldown: Duration,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stream: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rtsp_client: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub no_audio: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub audio_codec: String,
}

impl Camera {
    pub fn normalize(&mut self) {
        self.name = self.name.trim().to_string();
        self.host = self.host.trim().to_string();
        self.user = self.user.trim().to_string();
        self.path = self.path.trim().to_string();
        self.scheme = self.scheme.trim().to_string();
        self.url = self.url.trim().to_string();
        self.rtsp_transport = self.rtsp_transport.trim().to_string();
        self.rtsp_auth = self.rtsp_auth.trim().to_ascii_lowercase();
        self.stream = self.stream.trim().to_string();
        self.rtsp_client = self.rtsp_client.trim().to_string();
        self.audio_codec = self.audio_codec.trim().to_string();

        if self.port == 0 {
            self.port = DEFAULT_PORT;
        }
        if self.scheme.is_empty() {
            self.scheme = DEFAULT_SCHEME.to_string();
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub cameras: Vec<Camera>,
    #[serde(default, skip_serializing_if = "SlackConfig::is_empty")]
    pub slack: SlackConfig,
}

impl Config {
    pub fn normalize(&mut self) {
        self.defaults.normalize();
        for cam in &mut self.cameras {
            cam.normalize();
        }
    }

    /// Inserts or updates a camera by case-insensitive name match.
    /// Returns `true` if a new entry was appended, or `false` if updated in-place.
    pub fn upsert_camera(&mut self, mut cam: Camera) -> bool {
        cam.normalize();
        for existing in &mut self.cameras {
            if existing.name.eq_ignore_ascii_case(&cam.name) {
                *existing = cam;
                return false;
            }
        }
        self.cameras.push(cam);
        true
    }

    /// Finds a camera by case-insensitive name match.
    pub fn find_camera(&self, name: &str) -> Option<Camera> {
        let needle = name.trim();
        if needle.is_empty() {
            return None;
        }
        self.cameras
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(needle))
            .cloned()
    }
}

fn config_base_dir() -> Result<PathBuf, QamError> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        let trimmed = xdg.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    let home = dirs::home_dir()
        .ok_or_else(|| QamError::Config("cannot determine home directory".to_string()))?;
    Ok(home.join(".config"))
}

/// Returns the primary default config path (`$XDG_CONFIG_HOME/qam/config.yaml` or `~/.config/qam/config.yaml`).
pub fn default_path() -> Result<PathBuf, QamError> {
    Ok(config_base_dir()?.join("qam").join("config.yaml"))
}

/// Returns the legacy fallback config path (`$XDG_CONFIG_HOME/camsnap/config.yaml` or `~/.config/camsnap/config.yaml`).
pub fn legacy_default_path() -> Result<PathBuf, QamError> {
    Ok(config_base_dir()?.join("camsnap").join("config.yaml"))
}

/// Resolves the path to load when `--config` is optional, falling back to `camsnap/config.yaml` if `qam/config.yaml` does not exist.
pub fn resolve_load_path(explicit_path: &str) -> Result<PathBuf, QamError> {
    let trimmed = explicit_path.trim();
    if !trimmed.is_empty() {
        return Ok(PathBuf::from(trimmed));
    }
    let primary = default_path()?;
    if primary.exists() {
        return Ok(primary);
    }
    let legacy = legacy_default_path()?;
    if legacy.exists() {
        return Ok(legacy);
    }
    Ok(primary)
}

/// Loads a configuration from `path` (or the default path if `path` is empty).
/// Returns an empty `Config` if the file does not exist.
pub fn load(path: &str) -> Result<Config, QamError> {
    let resolved = resolve_load_path(path)?;
    load_from_path(&resolved)
}

pub fn load_from_path(path: &Path) -> Result<Config, QamError> {
    let data = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(e) => return Err(QamError::Config(format!("read config: {e}"))),
    };
    if data.trim().is_empty() {
        return Ok(Config::default());
    }
    let mut cfg: Config =
        serde_yaml::from_str(&data).map_err(|e| QamError::Config(format!("parse config: {e}")))?;
    cfg.normalize();
    Ok(cfg)
}

/// Saves `cfg` to `path` (or `default_path()` if `path` is empty), creating parent directories
/// with `0700` permissions and writing the file with `0600` permissions on Unix.
pub fn save(path: &str, cfg: &Config) -> Result<PathBuf, QamError> {
    let resolved = if path.trim().is_empty() {
        default_path()?
    } else {
        PathBuf::from(path.trim())
    };
    save_to_path(&resolved, cfg)?;
    Ok(resolved)
}

pub fn save_to_path(path: &Path, cfg: &Config) -> Result<(), QamError> {
    let mut normalized = cfg.clone();
    normalized.normalize();

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| QamError::Config(format!("mkdir config dir: {e}")))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }

    let yaml = serde_yaml::to_string(&normalized)
        .map_err(|e| QamError::Config(format!("marshal config: {e}")))?;

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| QamError::Config(format!("write config: {e}")))?;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|e| QamError::Config(format!("chmod config: {e}")))?;
        f.write_all(yaml.as_bytes())
            .map_err(|e| QamError::Config(format!("write config: {e}")))?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, yaml.as_bytes())
            .map_err(|e| QamError::Config(format!("write config: {e}")))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn default_path_respects_xdg_and_falls_back_to_camsnap() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let xdg_dir = tmp.path().to_str().unwrap();
        // SAFETY: serialized by ENV_LOCK in unit tests
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", xdg_dir);
        }

        let primary = default_path().unwrap();
        assert_eq!(primary, tmp.path().join("qam").join("config.yaml"));

        // Write a legacy camsnap config and verify load("") falls back to it when qam/config.yaml is absent
        let legacy_path = legacy_default_path().unwrap();
        let mut legacy_cfg = Config::default();
        legacy_cfg.upsert_camera(Camera {
            name: "legacy_cam".into(),
            host: "10.0.0.9".into(),
            ..Default::default()
        });
        save_to_path(&legacy_path, &legacy_cfg).unwrap();

        let loaded = load("").unwrap();
        assert_eq!(loaded.cameras.len(), 1);
        assert_eq!(loaded.cameras[0].name, "legacy_cam");
    }

    #[test]
    fn load_missing_returns_empty_config() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("missing.yaml");
        let cfg = load(missing.to_str().unwrap()).unwrap();
        assert!(cfg.cameras.is_empty());
        assert!(cfg.defaults.is_empty());
    }

    #[test]
    fn save_permissions_are_0600() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg_path = tmp.path().join("nested").join("config.yaml");
        save_to_path(&cfg_path, &Config::default()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(&cfg_path).unwrap();
            assert_eq!(meta.permissions().mode() & 0o777, 0o600);
            let dir_meta = std::fs::metadata(cfg_path.parent().unwrap()).unwrap();
            assert_eq!(dir_meta.permissions().mode() & 0o777, 0o700);
        }
    }

    #[test]
    fn upsert_and_find_are_case_insensitive() {
        let mut cfg = Config::default();
        let created = cfg.upsert_camera(Camera {
            name: "Kitchen".into(),
            host: "192.168.1.10".into(),
            ..Default::default()
        });
        assert!(created);
        assert_eq!(cfg.cameras.len(), 1);
        assert_eq!(cfg.cameras[0].port, 554);
        assert_eq!(cfg.cameras[0].scheme, "rtsp");

        let created2 = cfg.upsert_camera(Camera {
            name: "kitchen".into(),
            host: "192.168.1.20".into(),
            ..Default::default()
        });
        assert!(!created2);
        assert_eq!(cfg.cameras.len(), 1);
        assert_eq!(cfg.cameras[0].host, "192.168.1.20");

        let found = cfg.find_camera("KITCHEN").expect("should find camera");
        assert_eq!(found.host, "192.168.1.20");
        assert!(cfg.find_camera("nonexistent").is_none());
    }

    #[test]
    fn yaml_roundtrip_go_compat() {
        let go_yaml = r#"
defaults:
  rtsp_transport: udp
  rtsp_auth: digest
  timeout: 15s
  threshold: 0.3
  clip_duration: 5s
  cooldown: 12s
cameras:
  - name: front
    host: 192.168.1.10
    port: 554
    user: admin
    pass: secret
    path: /h264Preview_01_main
    scheme: rtsp
    rtsp_transport: tcp
    rtsp_auth: basic
    threshold: 0.2
    clip_duration: 8s
    cooldown: 10s
  - name: legacy
    host: 192.168.1.11
    protocol: rtsps
    username: tapo
    password: pw
    stream: stream2
    no_audio: true
"#;
        let mut cfg: Config = serde_yaml::from_str(go_yaml).unwrap();
        cfg.normalize();
        assert_eq!(cfg.defaults.rtsp_transport, "udp");
        assert_eq!(cfg.defaults.rtsp_auth, "digest");
        assert_eq!(cfg.defaults.timeout, Duration::from_secs(15));
        assert_eq!(cfg.defaults.clip_duration, Duration::from_secs(5));
        assert_eq!(cfg.defaults.cooldown, Duration::from_secs(12));
        assert_eq!(cfg.cameras.len(), 2);
        assert_eq!(cfg.cameras[0].user, "admin");
        assert_eq!(cfg.cameras[0].pass, "secret");
        assert_eq!(cfg.cameras[0].clip_duration, Duration::from_secs(8));
        assert_eq!(cfg.cameras[1].scheme, "rtsps");
        assert_eq!(cfg.cameras[1].user, "tapo");
        assert_eq!(cfg.cameras[1].pass, "pw");
        assert_eq!(cfg.cameras[1].stream, "stream2");
        assert!(cfg.cameras[1].no_audio);

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.yaml");
        save_to_path(&path, &cfg).unwrap();
        let reloaded = load_from_path(&path).unwrap();
        assert_eq!(reloaded, cfg);
    }
}
