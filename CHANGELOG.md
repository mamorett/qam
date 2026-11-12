# Changelog

All notable changes to `qam` will be documented in this file.

## [0.4.0] - 2026-11-12

### Rewritten in 100% Pure Rust (`qam`)
- **Zero Runtime Dependencies**: Completely eliminated all runtime and build-time dependencies on `ffmpeg`, `ffprobe`, `libav*`, and OpenSSL. Self-contained single binary with statically compiled OpenH264, pure-Rust H.265 (`rust_h265`), native MP4 muxer, and `rustls`.
- **RTSP Engine**: Replaced `gortsplib`/`ffmpeg` with pure-Rust `retina` RTSP engine supporting interleaved TCP, RTP over UDP, and `rtsps://` over TLS with self-signed certificate leniency.
- **Decoding & Image Encoding**: Hardware-free software decode for H.264, H.265, and MJPEG. Atomic image file output supporting JPEG, PNG, and WebP.
- **Muxing**: Stream-copy video recording into MP4 with faststart, and audio recording into WAV (PCM s16le).
- **Motion Detection**: Bit-accurate mean absolute frame difference (MAFD) scene-change detector matching FFmpeg's `select='gt(scene,T)'` without spawning any external process.
- **ONVIF WS-Discovery**: Pure-Rust UDP multicast discovery probe and SOAP GetDeviceInformation with WS-Security digest and Basic fallback.
- **Slack Integration**: Modern 3-step file upload API (`files.getUploadURLExternal` -> POST -> `files.completeUploadExternal`) with channel/user resolution.
- **Bug Fixes from camsnap v0.3.0**:
  - Q1: RTSP transport/client flags now respect flag > camera > default precedence.
  - Q2: Fixed `slack set` without `--config` crashing when writing default path.
  - Q3: Fixed `discover --info` context timeout expiration.
  - Q4: Fixed `clip --timeout` killing long `--dur` recordings.
  - Q5: Watch action inherits full environment (including PATH and HOME) in addition to `QAM_*` and legacy `CAMSNAP_*` variables.
  - Q6: Clean WAV output with automatic PCM transcoding when source audio codec cannot be stored natively.
  - Q7: Fixed path concatenation duplicating segments.
  - Q8: Implemented real `--rtsp-auth` policy enforcement in RTSP session layer.
