# Technical Specification: `qam`

`qam` is a pure-Rust application for capturing snapshots, recording clips, and monitoring motion from RTSP cameras.

## Architectural Layers

1. **CLI Layer (`src/cli/`)**:
   - `root.rs`: Command line parsing via `clap`, exit codes, styling, global error handling.
   - Subcommand handlers: `add.rs`, `list.rs`, `snap.rs`, `clip.rs`, `audio.rs`, `watch.rs`, `discover.rs`, `doctor.rs`, `slack_cmd.rs`, `version.rs`, `completion.rs`.
   - `flags.rs`: Parsers for Go-compatible durations, boolean flags, and RTSP transport/auth flags.
   - `resolve.rs`: Precedence resolution, template substitution, and URL construction.

2. **Configuration & Data Layer (`src/config.rs`, `src/rtsp_url.rs`)**:
   - YAML loading and saving with secure permissions (0600 file, 0700 dir).
   - Seamless fallback from `$XDG_CONFIG_HOME/qam/config.yaml` to `$XDG_CONFIG_HOME/camsnap/config.yaml`.
   - Robust URL building with credential percent-encoding and redaction.

3. **RTSP Media Session Engine (`src/media/`)**:
   - `session.rs`: Pure-Rust RTSP client wrapping `vendor/retina` with custom patches for `rtsps://` TLS, auth policy enforcement, and Opus.
   - `annexb.rs`: NAL unit parsing, Annex-B <-> AVCC bitstream conversion, SPS/PPS extraction, keyframe detection.
   - `video_decode.rs`: OpenH264, pure-Rust H.265 (`rust_h265`), and MJPEG software decoders. Fast luma extraction for motion detection.
   - `yuv.rs`: Fixed-point YUV420p to RGB conversion.
   - `image_out.rs`: Quality-mapped JPEG, PNG, and WebP atomic writers.
   - `scene.rs`: Mean Absolute Frame Difference (MAFD) motion detector matching FFmpeg `select='gt(scene,T)'`.
   - `recorder.rs` & `mux/`: Native MP4 and WAV writers.
   - `probe.rs`: Non-blocking 3-attempt RTSP diagnostic probe with typed error classification.

4. **Integration Layer**:
   - `src/discovery/`: ONVIF WS-Discovery UDP multicast probe and SOAP device info queries.
   - `src/slack.rs`: Slack Web API client implementing 3-step file upload flow.
   - `src/signals.rs`: Clean SIGINT/SIGTERM shutdown, container finalization, and exit code 130.
