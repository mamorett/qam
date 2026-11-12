---
name: qam
description: Capture snapshots, record video clips, and monitor motion from RTSP/ONVIF cameras with zero ffmpeg dependencies. Use when the user wants to interact with cameras, discover new cameras, set up motion alerts, or send media to Slack.
---

# Qam Skill

`qam` is a fast, self-contained pure-Rust CLI tool for capturing frames and clips from RTSP cameras. It requires zero external tools (no FFmpeg, GStreamer, or OpenSSL), supports persistence for camera configurations, and integrates with Slack for real-time alerting.

## Core Capabilities

### 1. Camera Management
- **`add`**: Configure a new camera with RTSP credentials, transport settings, and stream selection.
- **`list`** (alias `ls`): Display all registered cameras and their current configuration.
- **`discover`**: Find ONVIF-compatible cameras on your network via WS-Discovery.

**Examples:**
```sh
# Add a Tapo camera
qam add --name kitchen --host 192.168.0.100 --user admin --pass secret --rtsp-transport udp --stream stream2

# List existing cameras
qam list
```

### 2. Media Capture
- **`snap`**: Grab a single frame from a camera (JPEG/PNG/WebP).
- **`clip`**: Record a short MP4 clip (e.g., 5-10s).
- **`audio`** (alias `record-audio`): Record an audio-only clip to WAV (PCM s16le).
- **Slack Integration**: Use the `--slack-channel` and `--slack-message` flags to send media directly to Slack.

**Examples:**
```sh
# Capture a snapshot to file
qam snap kitchen --out /tmp/snapshot.jpg

# Record a 5s clip and send to Slack
qam clip kitchen --dur 5s --slack-channel #security --slack-message "Motion detected!"

# Record 10s of audio only to a WAV file
qam audio kitchen --seconds 10 --out /tmp/audio.wav
```

### 3. Motion Monitoring
- **`watch`**: Uses built-in pure-Rust scene-change detection to monitor for motion without FFmpeg.
- **Actions**: Trigger any shell command when motion is detected. Environment variables `QAM_CAMERA`, `QAM_SCORE`, and `QAM_TIME` (as well as legacy `CAMSNAP_*` variables) are available to the action script.

**Examples:**
```sh
# Monitor kitchen for motion (0.2 threshold) and trigger a snap
qam watch kitchen --threshold 0.2 --cooldown 10s --action 'qam snap kitchen --slack-channel #alerts'
```

### 4. Diagnostics
- **`doctor`**: Checks built-in media engine status, network reachability, and probes RTSP streams with error classification.
