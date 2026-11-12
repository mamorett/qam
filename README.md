# qam

One command to grab RTSP frames, clips, or motion alerts from your cameras.
Rewritten in 100% pure Rust: **zero external dependencies, zero ffmpeg, zero OpenSSL**.

## Features

- **Self-contained**: Single static binary with built-in H.264, H.265, and MJPEG decoders, MP4/WAV muxers, and pure-Rust TLS (`rustls`). No FFmpeg, ffprobe, or libav runtime dependencies.
- **RTSP & RTSPS**: Interleaved TCP, RTP over UDP, and TLS (`rtsps://`) with self-signed certificate leniency.
- **Snapshots**: Capture single frames to JPEG, PNG, or WebP with configurable quality.
- **Recordings**: Stream-copied MP4 video recordings and clean WAV audio recordings.
- **Motion Detection**: Bit-compatible scene change detection matching FFmpeg `select='gt(scene,T)'`.
- **ONVIF WS-Discovery**: Discover local cameras and fetch device information via SOAP.
- **Slack Alerts**: Upload snapshots and clips directly to Slack channels or users.
- **Backward Compatible**: Compatible with `camsnap` configurations, environment variables, and CLI syntax.

## Installation

### From Source

```sh
cargo install --path .
```

Or build with `make`:

```sh
make install
```

## Quick Start

```sh
# Discover cameras on your local network
qam discover

# Add a camera
qam add --name porch --host 192.168.1.100 --user admin --pass secret --rtsp-transport tcp

# Take a snapshot
qam snap porch --out porch.jpg

# Record a 5-second video clip
qam clip porch --dur 5s --out porch.mp4

# Record a 10-second audio clip
qam audio porch --dur 10s --out audio.wav

# Run motion detection and trigger an action
qam watch porch --threshold 0.2 --cooldown 5s --action 'qam snap porch --slack-channel #alerts'

# Run diagnostics
qam doctor --probe
```

## License

MIT License. See [LICENSE](LICENSE) for details.
