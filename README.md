<div align="center">
  <img src="logo.png" alt="qam logo" width="400" />

  # qam

  **One command to grab RTSP frames, video clips, audio, or motion alerts from your cameras.**

  *100% Pure Rust • Zero FFmpeg • Zero GStreamer • Zero OpenSSL • Zero Runtime Dependencies*

  <p align="center">
    <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT"></a>
    <a href="Cargo.toml"><img src="https://img.shields.io/badge/Rust-2024%20Edition%20(1.89%2B)-orange.svg" alt="Rust 2024 Edition"></a>
    <a href="#architecture--zero-dependencies"><img src="https://img.shields.io/badge/dependencies-zero%20ffmpeg-brightgreen.svg" alt="Zero External Dependencies"></a>
    <a href="#backward-compatibility"><img src="https://img.shields.io/badge/camsnap-compatible-purple.svg" alt="Camsnap Compatible"></a>
  </p>
</div>

---

## Table of Contents

- [Overview](#overview)
- [Key Features](#key-features)
- [Installation](#installation)
  - [Prerequisites](#prerequisites)
  - [Building from Source](#building-from-source)
- [Quick Start](#quick-start)
- [CLI Reference](#cli-reference)
  - [`qam discover` — ONVIF WS-Discovery](#qam-discover)
  - [`qam add` — Register Camera](#qam-add)
  - [`qam list` / `ls` — List Cameras](#qam-list--ls)
  - [`qam snap` — Capture Snapshot](#qam-snap)
  - [`qam clip` — Record Video Clip](#qam-clip)
  - [`qam audio` — Record Audio Clip](#qam-audio)
  - [`qam watch` — Motion Detection & Actions](#qam-watch)
  - [`qam doctor` — Diagnostic Probing](#qam-doctor)
  - [`qam slack` — Manage Slack Integration](#qam-slack)
  - [`qam completion` — Shell Autocompletions](#qam-completion)
- [Camera Setup & Compatibility](#camera-setup--compatibility)
  - [TP-Link Tapo](#tp-link-tapo)
  - [UniFi Protect (Ubiquiti)](#unifi-protect-ubiquiti)
  - [Reolink, Hikvision, Dahua & Generic RTSP](#reolink-hikvision-dahua--generic-rtsp)
- [Configuration](#configuration)
  - [Config File Locations](#config-file-locations)
  - [Annotated Example (`config.yaml`)](#annotated-example-configyaml)
- [Environment Variables](#environment-variables)
- [Architecture & Zero Dependencies](#architecture--zero-dependencies)
- [Backward Compatibility with `camsnap`](#backward-compatibility)
- [License](#license)

---

## Overview

`qam` is a blazing-fast, self-contained CLI media engine for RTSP and ONVIF IP security cameras.

Unlike traditional camera tools that shell out to bulky external binaries like `ffmpeg`, `ffprobe`, or `gstreamer`, `qam` is engineered from the ground up in **100% pure Rust**. It embeds its own video decoders, audio transcoder, MP4 faststart muxer, WAV writer, TLS engine (`rustls`), and a bit-accurate scene change detector into a single static binary.

You can run `qam` in bare minimal environments, scratch Docker containers, or constrained systems with `PATH=""`—it has zero dynamic dependencies on FFmpeg or OpenSSL.

---

## Key Features

| Feature | Description |
| :--- | :--- |
| **Zero Runtime Dependencies** | Single self-contained binary. No FFmpeg, no libav*, no OpenSSL, no GStreamer needed. |
| **RTSP & RTSPS Protocols** | Supports TCP interleaved (default), UDP RTP, and TLS encrypted `rtsps://` with self-signed certificate leniency. |
| **Multi-Codec Video Decode** | Hardware-independent software decoding for **H.264** (embedded OpenH264), **H.265 / HEVC** (`rust_h265`), and **MJPEG**. |
| **High-Quality Snapshots** | Capture single frames to **JPEG**, **PNG**, or **WebP** formats with atomic writes. |
| **Stream-Copied Video Clips** | Remux video frames instantly into standard **MP4** containers with `moov` atom upfront (faststart) for immediate web playback. |
| **Clean Audio Recording** | Record audio to **WAV** (PCM s16le), with automatic decoding of AAC, PCMU, PCMA, and L16 streams. |
| **Bit-Accurate Motion Detection** | Pure-Rust Mean Absolute Frame Difference (MAFD) detector bit-compatible with FFmpeg's `select='gt(scene,T)'`. |
| **Event Triggers & Action Hooks** | Execute shell actions or templated commands (`{camera}`, `{score}`, `{time}`) on motion with cooldown and optional JSON event logging. |
| **ONVIF WS-Discovery** | Discover local cameras automatically over UDP multicast (`239.255.255.250:3702`) and inspect device details via SOAP. |
| **Direct Slack Alerts** | Post snapshots and video recordings directly to Slack channels (`#channel`) or direct messages (`@user`) via the modern 3-step Slack upload API. |
| **Stream Diagnostics** | `qam doctor` tests network reachability, validates configurations, and probes RTSP handshakes with typed error reporting. |

---

## Installation

### Prerequisites

- **Rust & Cargo**: Version **1.89.0** or newer (Rust 2024 edition).
  ```sh
  rustc --version
  cargo --version
  ```
  Update if necessary:
  ```sh
  rustup update stable
  ```
- **C/C++ Compiler**: A standard C/C++ compiler (`clang` or `gcc`) used exclusively during compilation for the embedded OpenH264 source. No external multimedia libraries or dev headers are required.

### Building from Source

Install directly using `cargo`:

```sh
cargo install --path .
```

Or build and install via `make` into `~/bin/`:

```sh
make install
```

To run formatting, linter, tests, and dependency verification:

```sh
make all
```

---

## Quick Start

```sh
# 1. Discover local cameras on your network
qam discover

# 2. Register a camera
qam add --name porch --host 192.168.1.100 --user admin --pass secret --rtsp-transport tcp

# 3. Capture a snapshot (JPEG, PNG, or WebP)
qam snap porch --out porch.jpg

# 4. Record a 5-second MP4 video clip
qam clip porch --dur 5s --out porch.mp4

# 5. Record 10 seconds of audio to WAV
qam audio porch --dur 10s --out audio.wav

# 6. Monitor for motion and dispatch alerts
qam watch porch --threshold 0.2 --cooldown 5s --action 'qam snap porch --slack-channel #alerts'

# 7. Check camera reachability & stream health
qam doctor --probe
```

---

## CLI Reference

### `qam discover`

Scans the local subnet for ONVIF-compliant devices using WS-Discovery UDP multicast.

```sh
# Standard discovery (3s timeout)
qam discover

# Extended timeout
qam discover --timeout 5s

# Query device information (Model, Firmware, Serial) via ONVIF SOAP
qam discover --info
```

### `qam add`

Saves or updates camera credentials and settings in your configuration file.

```sh
qam add \
  --name kitchen \
  --host 192.168.0.120 \
  --port 554 \
  --protocol rtsp \
  --user admin \
  --pass secret \
  --rtsp-transport tcp \
  --stream stream1
```

**Common Flags:**
- `--name`: Unique alias for the camera (case-insensitive).
- `--host`: Hostname or IP address.
- `--port`: RTSP port (default: `554`).
- `--protocol`: Protocol scheme (`rtsp` or `rtsps`, default: `rtsp`).
- `--user` / `--pass`: Camera credentials.
- `--stream`: Stream name or path segment (e.g. `stream1` for HD, `stream2` for SD).
- `--path`: Explicit custom RTSP path (e.g. UniFi token `/Bfy...`).
- `--rtsp-transport`: Preferred transport (`tcp` or `udp`).
- `--no-audio`: Disable audio capture by default for this camera.
- `--audio-codec`: Preferred audio codec (e.g. `copy`, `aac`).

### `qam list` / `ls`

Lists all cameras saved in the configuration file.

```sh
qam list
# Or using the alias:
qam ls
```

### `qam snap`

Grabs a single video keyframe, decodes it, and atomically writes it to disk or uploads it to Slack. The output image format is determined automatically by extension (`.jpg`, `.jpeg`, `.png`, `.webp`).

```sh
# Save as JPEG
qam snap kitchen --out snapshot.jpg

# Save as PNG or WebP
qam snap kitchen --out snapshot.png
qam snap kitchen --out snapshot.webp

# Direct upload to Slack
qam snap kitchen --slack-channel #security --slack-message "Porch snapshot"

# Override stream or transport on the fly
qam snap kitchen --stream stream2 --rtsp-transport udp --out preview.jpg
```

### `qam clip`

Records a short video clip by stream-copying H.264/H.265 video and audio directly into an MP4 container with faststart (`moov` at beginning).

```sh
# Record 10s clip (default duration)
qam clip kitchen --out kitchen.mp4

# Record 5s clip without audio
qam clip kitchen --dur 5s --no-audio --out silent.mp4

# Record using integer seconds flag
qam clip kitchen -s 15 --out long.mp4

# Record and upload directly to Slack
qam clip kitchen --dur 5s --slack-channel #alerts --slack-message "Motion clip"
```

### `qam audio`

Records audio directly from the camera's RTSP audio track into a pristine WAV file (PCM s16le). Automatically decodes AAC, PCMU, PCMA, or L16 streams.

```sh
# Record 10s of audio (alias: qam record-audio)
qam audio kitchen --dur 10s --out ambient.wav

# Specify duration in seconds
qam audio kitchen -s 30 --out speech.wav

# Send audio clip to Slack
qam audio kitchen --dur 10s --slack-channel #audio-logs
```

### `qam watch`

Runs continuous real-time motion detection using pure-Rust Mean Absolute Frame Difference (MAFD). When the scene change score exceeds `--threshold`, it triggers your action command or template.

```sh
# Basic watch: trigger snapshot on motion
qam watch kitchen \
  --threshold 0.2 \
  --cooldown 5s \
  --action 'qam snap kitchen --out /tmp/motion.jpg'

# Using templated commands ({camera}, {score}, {time})
qam watch kitchen \
  --threshold 0.2 \
  --action-template 'echo "Camera {camera} triggered with score {score} at {time}"'

# Stream events in structured JSON Lines
qam watch kitchen --threshold 0.25 --json --action 'true'

# Run monitoring for a bounded duration (e.g., 10 minutes)
qam watch kitchen --threshold 0.2 --duration 10m --action './on_motion.sh'
```

**Environment Variables passed to `--action`:**
- `QAM_CAMERA` (legacy: `CAMSNAP_CAMERA`)
- `QAM_SCORE` (legacy: `CAMSNAP_SCORE`)
- `QAM_TIME` (legacy: `CAMSNAP_TIME`)

### `qam doctor`

Performs diagnostic checks: verifies media engine compilation, reads config files, tests network TCP reachability, and optionally probes live RTSP streams.

```sh
# Fast reachability check
qam doctor

# Full RTSP handshake & stream probe
qam doctor --probe

# Probe using UDP transport with custom timeout
qam doctor --probe --rtsp-transport udp --timeout 5s
```

### `qam slack`

Configure and verify Slack integration for automated snapshot and clip distribution.

```sh
# Persist Slack bot token and default channel
qam slack set --token "xoxb-your-token" --channel "#security"

# Test file upload
qam slack test ./snapshot.jpg --channel "#test" --message "Test upload from qam"
```

### `qam completion`

Outputs shell completion scripts to stdout for your preferred shell:

```sh
# Bash
source <(qam completion bash)

# Zsh
qam completion zsh > ~/.zsh/completion/_qam

# Fish
qam completion fish > ~/.config/fish/completions/qam.fish

# PowerShell
qam completion powershell | Out-String | Invoke-Expression
```

---

## Camera Setup & Compatibility

`qam` works with any camera supporting RTSP (H.264, H.265, MJPEG) or ONVIF Profile S/T.

### TP-Link Tapo
Tapo cameras (C100, C200, C210, C310, C320WS, etc.) host RTSP on port `554`:
- High Quality (1080p/2K/4K): `/stream1`
- Low Quality (360p): `/stream2`
- Enable the RTSP account in **Tapo App > Camera Settings > Advanced Settings > Camera Account**.
- Recommended command:
  ```sh
  qam add --name tapo --host 192.168.1.50 --user tapouser --pass tapopass --stream stream1 --rtsp-transport tcp
  ```

### UniFi Protect (Ubiquiti)
UniFi Protect cameras provide secure RTSPS URLs with randomized tokens:
- Copy the RTSP(S) link from UniFi Protect web interface (e.g., `rtsps://192.168.1.1:7441/Bfy...`).
- Pass the token path via `--path`:
  ```sh
  qam add --name protect --host 192.168.1.1 --port 7441 --protocol rtsps --path /Bfya920ZbC --rtsp-transport tcp
  ```

### Reolink, Hikvision, Dahua & Generic RTSP
- **Reolink**: Main stream `h264Preview_01_main`, Sub stream `h264Preview_01_sub`.
- **Hikvision / Dahua**: Port `554`, streams typically `/Streaming/Channels/101` or `/cam/realmonitor?channel=1&subtype=0`.
- Use `--path` for complex query strings or custom endpoints.

---

## Configuration

### Config File Locations

`qam` looks for its configuration file in the following order:
1. `--config <path>` (explicit CLI flag).
2. `$XDG_CONFIG_HOME/qam/config.yaml` or `~/.config/qam/config.yaml`.
3. Fallback: `$XDG_CONFIG_HOME/camsnap/config.yaml` or `~/.config/camsnap/config.yaml`.

Configuration files are saved with secure Unix file permissions (`0600` for the file, `0700` for the directory).

### Annotated Example (`config.yaml`)

```yaml
defaults:
  rtsp_transport: tcp      # tcp | udp
  rtsp_auth: auto          # auto | basic | digest
  timeout: 30s
  threshold: 0.2           # Scene detection threshold (0.0 to 1.0)
  clip_duration: 10s       # Default clip recording duration
  cooldown: 5s             # Cooldown period between motion triggers

slack:
  token: "xoxb-your-slack-bot-token"
  default_channel: "#security"

cameras:
  - name: porch
    host: 192.168.1.100
    port: 554
    scheme: rtsp
    user: admin
    pass: "supersecret"
    stream: stream1
    rtsp_transport: tcp
    no_audio: false

  - name: driveway
    host: 192.168.1.101
    port: 7441
    scheme: rtsps
    path: /s0M3Un1F1T0k3n
    rtsp_transport: tcp
    threshold: 0.15
```

---

## Environment Variables

| Variable | Description |
| :--- | :--- |
| `SLACK_TOKEN` | Slack Bot OAuth token (`xoxb-...`), overrides config file. |
| `XDG_CONFIG_HOME` | Custom base directory for user configuration files. |
| `QAM_CAMERA` | Camera name populated during `qam watch` action commands. |
| `QAM_SCORE` | Motion detection score (e.g. `0.245`) during `qam watch` actions. |
| `QAM_TIME` | RFC 3339 timestamp during `qam watch` actions. |
| `CAMSNAP_*` | Backward-compatible aliases for `QAM_CAMERA`, `QAM_SCORE`, and `QAM_TIME`. |

---

## Architecture & Zero Dependencies

```
┌────────────────────────────────────────────────────────┐
│                        qam CLI                         │
└───────────────────────────┬────────────────────────────┘
                            │
              ┌─────────────┴─────────────┐
              ▼                           ▼
    ┌──────────────────┐        ┌──────────────────┐
    │  ONVIF Discovery │        │   RTSP Session   │
    │  (UDP Multicast) │        │ (vendor/retina)  │
    └──────────────────┘        └─────────┬────────┘
                                          │
            ┌─────────────────────────────┼────────────────────────────┐
            ▼                             ▼                            ▼
   ┌─────────────────┐           ┌─────────────────┐          ┌─────────────────┐
   │  Video Decoder  │           │  Audio Decoder  │          │   MP4 Muxer     │
   │ H.264 / H.265 / │           │  AAC/PCM/PCMA/U │          │ (Stream-Copy &  │
   │      MJPEG      │           │   -> WAV Muxer  │          │    Faststart)   │
   └────────┬────────┘           └─────────────────┘          └─────────────────┘
            │
      ┌─────┴──────────────────────┐
      ▼                            ▼
┌─────────────┐             ┌─────────────┐
│    MAFD     │             │ Image Output│
│ Motion Det. │             │  JPEG / PNG │
│ (Scene Chg) │             │  / WebP     │
└─────────────┘             └─────────────┘
```

- **RTSP Protocol**: Managed via `retina` with customized extensions for TLS (`rtsps://` via `rustls`) and strict digest/basic authentication policy handling.
- **Bitstream Demuxing**: Annex-B to AVCC parsing, NAL unit separation, SPS/PPS extraction, and keyframe indexing.
- **Decoding**: Statically compiled OpenH264 (C/C++), `rust_h265` (pure Rust HEVC), and JPEG decoders.
- **Motion Engine**: Mean absolute difference calculated across consecutive luma planes matching FFmpeg's internal algorithms.
- **Zero dynamic multimedia linkage**: Verified via `./ci/check-no-ffmpeg.sh` and `make check-no-ffmpeg`.

---

## Backward Compatibility

`qam` is a drop-in successor to `camsnap`. It maintains 100% compatibility:
- Automatic reading of existing `~/.config/camsnap/config.yaml` if `qam/config.yaml` is not yet created.
- Full support for `CAMSNAP_SCORE`, `CAMSNAP_TIME`, and `CAMSNAP_CAMERA` in action scripts.
- CLI aliases (`ls` for `list`, `record-audio` for `audio`).
- Identical command line options, flags, and parameter semantics.

---

## License

`qam` is distributed under the terms of the **MIT License**. See [LICENSE](LICENSE) for details.
