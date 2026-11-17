---
name: qam
description: Capture snapshots, record video clips, and monitor motion from RTSP/ONVIF cameras with zero ffmpeg dependencies. Use when the user wants to interact with cameras, discover new cameras, set up motion alerts, or send media to Slack.
---

# Qam Skill

`qam` is a fast, self-contained pure-Rust CLI tool for capturing frames, clips, and audio from RTSP/ONVIF IP cameras. It requires zero external tools (no FFmpeg, GStreamer, or OpenSSL), supports persistence for camera configurations, and integrates with Slack for real-time alerting.

## Core Capabilities

### 1. Camera Management & Discovery
- **`add`**: Configure a new camera with RTSP credentials, transport settings, stream selection, or custom path.
  - `--name`: Unique identifier for the camera.
  - `--host`, `--port` (default 554), `--protocol` (`rtsp` or `rtsps`).
  - `--user`, `--pass`: Camera authentication credentials.
  - `--stream`: Stream name (e.g., `stream1` for HD, `stream2` for SD).
  - `--path`: Custom RTSP path segment (e.g., `/Bfy...` token from UniFi Protect). Overrides `--stream`.
  - `--rtsp-transport`: Preferred transport (`tcp` or `udp`).
  - `--no-audio`: Flag to omit audio track by default.
  - `--audio-codec`: Preferred audio codec (e.g., `copy`, `aac`).
- **`list`** (alias `ls`): Display all registered cameras and their current configuration.
- **`discover`**: Find ONVIF-compatible cameras on the local network via WS-Discovery UDP multicast.
  - `--timeout`: Discovery wait time (default `3s`).
  - `--info`: Queries ONVIF `GetDeviceInformation` for model and firmware details.

**Examples:**
```sh
# Discover cameras on the local network
qam discover --info

# Add a standard Tapo camera
qam add --name kitchen --host 192.168.0.100 --user admin --pass secret --rtsp-transport tcp --stream stream2

# Add a UniFi Protect RTSPS camera
qam add --name driveway --host 192.168.1.1 --port 7441 --protocol rtsps --path /Bfya920ZbC --rtsp-transport tcp

# List saved cameras
qam list
```

### 2. Media Capture
- **`snap`**: Grab a single keyframe to file or upload to Slack.
  - Format is deduced from output file extension: `.jpg` / `.jpeg`, `.png`, `.webp`.
  - `--out`: Target output filepath.
  - `--stream` / `--path`: Override stream path on the fly.
  - `--rtsp-transport`: Force transport (`tcp` or `udp`).
  - `--slack` / `--slack-channel`: Upload to Slack (default channel from config, or `#alerts` / `@user`).
  - `--slack-message`: Message posted alongside the image; requires `--slack`.
  - `--s3` / `--s3-bucket`: Upload to S3-compatible object storage (optional `--s3-key`, `--s3-prefix`, `--s3-endpoint`, `--s3-region`).
  - `--timeout`: Capture timeout (default `30s`).
- **`clip`**: Record a short stream-copied MP4 clip with upfront `moov` atom (faststart).
  - `--dur`: Duration string (e.g. `5s`, `10s`).
  - `-s`, `--seconds`: Duration in integer seconds.
  - `--no-audio`: Omit audio track from the recording.
  - `--audio-only`: Record audio track only.
  - `--slack`, `--slack-channel`, `--slack-message`: Send clip directly to Slack.
  - `--s3`, `--s3-bucket`, `--s3-key`: Upload clip directly to S3-compatible storage.
- **`audio`** (alias `record-audio`): Record an audio-only clip to WAV (PCM s16le).
  - Automatically decodes AAC, PCMU, PCMA, and L16 streams.
  - `--dur` / `-s`: Duration (default `10s`).
  - `--out`: Target WAV output path.
  - Supports direct Slack (`--slack`) and S3 (`--s3`) upload flags.

**Examples:**
```sh
# Capture a snapshot to JPEG, PNG, or WebP
qam snap kitchen --out /tmp/kitchen.jpg
qam snap kitchen --out /tmp/kitchen.webp

# Record a 5-second video clip
qam clip kitchen --dur 5s --out /tmp/kitchen.mp4

# Record a silent 10s video clip
qam clip kitchen --dur 10s --no-audio --out /tmp/silent.mp4

# Record 10s of audio only to WAV
qam audio kitchen --seconds 10 --out /tmp/ambient.wav

# Snap and upload directly to Slack
qam snap kitchen --slack-channel #security --slack-message "Porch check!"
```

### 3. Motion Monitoring
- **`watch`**: Real-time scene-change detection using pure-Rust Mean Absolute Frame Difference (MAFD).
  - `--threshold`: Sensitivity threshold between 0.0 and 1.0 (default `0.2`, higher = less sensitive).
  - `--cooldown`: Min time between triggers (default `5s`).
  - `--duration`: Max monitoring runtime (default `0s` = run indefinitely until interrupted).
  - `--action`: Shell command executed upon motion.
  - `--action-template`: Command template supporting placeholders `{camera}`, `{score}`, `{time}`.
  - `--json`: Output motion events as structured JSON Lines.
  - Spawned actions inherit `QAM_CAMERA`, `QAM_SCORE`, `QAM_TIME` (and `CAMSNAP_*` legacy equivalents).

**Examples:**
```sh
# Monitor kitchen with snapshot trigger
qam watch kitchen --threshold 0.2 --cooldown 5s --action 'qam snap kitchen --slack-channel #alerts'

# Monitor with template command
qam watch kitchen --threshold 0.2 --action-template 'echo "Camera {camera} triggered with score {score} at {time}"'

# Stream JSON lines for 5 minutes
qam watch kitchen --threshold 0.2 --duration 5m --json --action 'true'
```

### 4. Diagnostics & Probing
- **`doctor`**: Checks built-in media engine status, configuration existence, and camera network reachability.
  - `--probe`: Performs full RTSP handshake, SDP exchange, and stream validation.
  - `--timeout`: Timeout per camera (default `2s`).
  - `--rtsp-transport`: Force transport (`tcp` or `udp`) during probe.

**Examples:**
```sh
# Basic TCP reachability
qam doctor

# Deep RTSP probe
qam doctor --probe
```

### 5. Slack & S3 Management
- **`slack set`**: Store Slack bot token and default channel into configuration (used when `--slack` is passed).
  ```sh
  qam slack set --token "xoxb-your-token" --channel "#security"
  ```
- **`slack test`**: Validate Slack upload functionality with a test file.
  ```sh
  qam slack test ./snapshot.jpg --channel "#test" --message "Test upload"
  ```
- **`s3 set`**: Store S3-compatible endpoint, region, bucket, and credentials in configuration (used when `--s3` is passed).
  ```sh
  qam s3 set --endpoint "http://127.0.0.1:9000" --bucket "my-cams" --access-key-id "minio" --secret-access-key "minio123"
  ```
- **`s3 test`**: Upload a test file and verify it with a `HEAD` request.
  ```sh
  qam s3 test ./snapshot.jpg --bucket "my-cams"
  ```

### 6. Shell Completion
- **`completion`**: Generate shell autocomplete scripts.
  ```sh
  qam completion bash
  qam completion zsh
  qam completion fish
  qam completion powershell
  ```

## Configuration & Precedence

1. **Config file locations**:
   - Explicit flag: `--config <path>`
   - Default: `$XDG_CONFIG_HOME/qam/config.yaml` or `~/.config/qam/config.yaml`
   - Fallback: `$XDG_CONFIG_HOME/camsnap/config.yaml` or `~/.config/camsnap/config.yaml`
2. **Environment Variables**:
   - `SLACK_TOKEN`: Overrides config file Slack token.
   - `XDG_CONFIG_HOME`: Custom base configuration directory.
   - `QAM_CAMERA`, `QAM_SCORE`, `QAM_TIME`: Set during `watch` action executions.
