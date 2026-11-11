# Architecture Decisions (`docs/rust-decisions.md`)

This document records the Phase 0 spike findings and architecture decisions for porting `camsnap` v0.3.0 (Go) to `qam` v0.4.0 (Rust).

## ADR-001: Binary/Crate Naming (`qam`) and Backward Compatibility with `camsnap`

- **Decision**: Name the Rust crate and binary `qam` (`version = "0.4.0"`), with primary config path `~/.config/qam/config.yaml` (or `$XDG_CONFIG_HOME/qam/config.yaml`), temp file prefixes `qam-*.jpg` / `qam-*.mp4` / `qam-*.wav`, build-time version override `QAM_VERSION`, and watch action environment variables `QAM_CAMERA`, `QAM_SCORE`, `QAM_TIME`.
- **Backward Compatibility**:
  1. **Config path fallback**: When loading the default config path (i.e., `--config` is not explicitly passed), if `$XDG_CONFIG_HOME/qam/config.yaml` (or `~/.config/qam/config.yaml`) does not exist, `qam` checks `$XDG_CONFIG_HOME/camsnap/config.yaml` (or `~/.config/camsnap/config.yaml`) and loads it if present. Saving writes to the `qam` path unless `--config` overrides it.
  2. **Build-time version env**: `build.rs` checks `QAM_VERSION`, falling back to `CAMSNAP_VERSION`, then `CARGO_PKG_VERSION`.
  3. **Watch action env vars**: `qam watch --action` populates both `QAM_CAMERA` / `QAM_SCORE` / `QAM_TIME` and legacy `CAMSNAP_CAMERA` / `CAMSNAP_SCORE` / `CAMSNAP_TIME`.

## ADR-002: RTSP Client & RTP Depacketization (`vendor/retina`)

- **Audit of `retina 0.4.21`**:
  - Built-in RTP depacketizers:
    - Video: H.264 (`video/h264`), H.265 (`video/h265`, `h265` feature enabled by default), MJPEG (`video/jpeg`, `image/jpeg`).
    - Audio: AAC (`audio/mpeg4-generic`), G.711 A-law/μ-law (`audio/pcma`, `audio/pcmu`), G.726 (`audio/g726-16`, `audio/g726-24`, `audio/g726-32`, `audio/g726-40`), L16 (`audio/l16`), DVI4, G.722, G.723, u8.
  - Built-in framing modes: `FrameFormat::SIMPLE` (Annex-B NALs with inline SPS/PPS/VPS on keyframes, ADTS AAC) and `FrameFormat::MP4` (4-byte length-prefixed NALs, raw AAC).
  - Gaps identified in upstream `retina 0.4.21`:
    1. **RTSPS (`rtsps://` over TLS)**: Upstream `RtspConnection::validate_url` rejected schemes other than `"rtsp"`, and `src/tokio.rs` `Connection` wrapped a plain `TcpStream`.
    2. **Auth policy enforcement (`--rtsp-auth auto|basic|digest`)**: Upstream `SessionOptions` delegated `WWW-Authenticate` directly to `http_auth::PasswordClient` without a policy filter for restricting to `Basic` or `Digest`, and did not have a direct fallback for simple `Basic` challenges.
    3. **Opus (`audio/opus`)**: Upstream `Depacketizer::new` lacked a match arm for `("audio", "opus")`.
- **Decision**: Vendor `retina 0.4.21` into `vendor/retina` (MIT OR Apache-2.0) with three targeted patches:
  1. `MaybeTlsStream` in `vendor/retina/src/tokio.rs` using `tokio-rustls` + `rustls` (`ring` crypto provider) with a lenient `ServerCertVerifier` (accepting self-signed camera certificates by default, matching `gortsplib` behavior), plus `rtsps://` URL validation (default port 322).
  2. `AuthPolicy` (`Auto`, `Basic`, `Digest`) and `RequestedAuth` (`HttpAuth`, `Basic`) in `vendor/retina/src/client/mod.rs`.
  3. `("audio", "opus")` packet-per-frame depacketizer in `vendor/retina/src/codec/{mod.rs,simple_audio.rs}`.
- **Verification**: Verified in `spikes/` against a loopback `tokio-rustls` RTSPS server with a self-signed `rcgen` certificate, plus unit checks for `AuthPolicy` and `audio/opus`.

## ADR-003: H.264 Decoder & Synthetic Test Encoder (`openh264`)

- **Evaluated**: `openh264 = "0.9.10"` (BSD-2-Clause, compiles Cisco OpenH264 from bundled C++ source via `cc` with `source` feature enabled by default).
- **Benchmark (Apple Silicon M-series, release profile)**:
  - 1920×1080 @ 15fps decode: **1.64 ms/frame (~609 fps)** — 40× faster than the 66 ms/frame real-time budget.
  - Also provides an in-process H.264 encoder (`openh264::encoder::Encoder`), allowing `tests/support/fixtures.rs` to generate deterministic synthetic H.264 streams in-memory with zero external tools.
- **Decision**: Use `openh264 = "0.9.10"` for H.264 decoding in `src/media/video_decode.rs` and synthetic H.264 test fixture generation in `tests/support/fixtures.rs`.

## ADR-004: H.265 / HEVC Decoder (`rust_h265`)

- **Evaluated**:
  1. `rust_h265 = "0.4.1"` (MIT OR Apache-2.0, 100% pure Rust, zero external crate dependencies, zero C/C++ FFI, supports Main, Main 10, Main Still Picture, and Format Range Extensions `profile_idc = 4` including `x265` intra streams).
  2. `rusty_h265 = "0.7.1"` (Apache-2.0, pure Rust, `#![forbid(unsafe_code)]`, supports Main / Main 10 / Main Still Picture, rejects `profile_idc = 4` Range Extensions).
  3. `libde265` (C++ FFI, LGPL-3 — rejected due to license and C++ cross-compilation complexity).
- **Benchmark (1920×1080, 15 frames, release profile)**:
  - `rust_h265 0.4.1`: **1.55 ms/frame (~644 fps)**, decodes both Main profile and Format Range Extensions (`profile_idc = 4`).
  - `rusty_h265 0.7.1`: **1.49 ms/frame (~670 fps)** on Main profile, but rejects `profile_idc = 4`.
- **Decision**: Select `rust_h265 = "0.4.1"` as the H.265 decoder. It is pure Rust (MIT/Apache-2.0), has zero C/FFI dependencies (making cross-compilation trivial across all 5 release targets), exceeds the 1080p@15fps real-time requirement by >40×, and supports Main, Main 10, and Range Extension profiles emitted by IP cameras.

## ADR-005: Cross-Compilation & Toolchain Matrix

- **Target Matrix**:
  - `aarch64-apple-darwin` (native macOS ARM64)
  - `x86_64-apple-darwin` (macOS x86_64)
  - `x86_64-unknown-linux-gnu` (Linux x86_64 via `cargo-zigbuild` or native runner)
  - `aarch64-unknown-linux-gnu` (Linux ARM64 via `cargo-zigbuild`)
  - `x86_64-pc-windows-msvc` (Windows x86_64 via GitHub Actions `windows-latest`)
- **Decision**: Because `rust_h265`, `symphonia`, `mp4`, `image`, `rustls`, and `vendor/retina` are 100% Rust and `openh264` compiles portable C++ via `cc` (`nasm` optional fallback to C routines), all 5 targets build cleanly without external system library dependencies (`pkg-config`, `libssl`, or `libav*`).

## ADR-006: Scene-Change Detection Parity (`tests/fixtures/scene_reference.json`)

- **Formula**: Matches `ffmpeg`'s `libavfilter/vf_select.c` (`get_scene_score`):
  - Per-pixel mean absolute difference on the Y (luma) plane: $\text{mafd} = \frac{1}{W \cdot H}\sum |Y_t - Y_{t-1}|$
  - $\text{diff} = |\text{mafd} - \text{prev\_mafd}|$
  - $\text{score} = \text{clamp}(\min(\text{mafd}, \text{diff}) / 100.0, 0.0, 1.0)$
- **Verification**: Generated `tests/fixtures/scene_reference.json` comparing `ffmpeg` `lavfi.scene_score` against `SceneDetector::feed` across 5 deterministic 320×180 YUV420p frames (`solid_black` → `gray_50` → `checkerboard` → `horizontal_gradient` → `solid_white`). Maximum absolute difference is `4.17e-7` (within `ffmpeg`'s 6-decimal printout precision, well below the `< 0.02` requirement).
