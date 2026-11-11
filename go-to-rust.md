# camsnap: Go → Rust Port Plan

> **Audience:** an autonomous coding agent (or a human following along).
> **Goal:** Rewrite `camsnap` in Rust with **100 % user-facing feature parity** and **zero dependency on ffmpeg** (no `ffmpeg` binary, no `libav*` libraries, no subprocess media tools). The result must be a **single self-contained binary**.
> **Rule:** Do not skip, defer or "simplify away" any feature listed in §3. If something looks impossible, stop and record it in `docs/rust-decisions.md` (see Phase 0). Do not drop it quietly.

---

## 0. How to use this document

1. Read §1–§5 completely before writing code. They describe what exists today and the hard constraints.
2. Run the phases in §8 **in order**. Every phase ends with acceptance criteria. Do not start the next phase until they all pass.
3. §3 (Feature Inventory) is the **traceability matrix**. When a feature is done, tick its row in a copy kept at `docs/port-status.md`. The port is finished only when every row is ticked.
4. §6 (Behavioral Spec) is the source of truth for exact strings, defaults, validation order and precedence. If this document and the Go source disagree, this document wins **only** where §7 ("Known Go quirks & decisions") says so explicitly. Otherwise, match the Go source.
5. Keep the Go tree untouched until Phase 12. You need it as a reference and for side-by-side comparison.

---

## 1. Current system overview (Go, v0.3.0)

| Go package | Responsibility | Lines |
|---|---|---|
| `cmd/camsnap/main.go` | Entrypoint; `version` var (`-X main.version`); prints error to stderr; exits with code 1 | 19 |
| `internal/cli/*` | Cobra command tree: `add`, `list`, `snap`, `clip`, `audio` (alias `record-audio`), `watch`, `discover`, `doctor`, `slack set`, `slack test`, `version`. Also helpers, styling and Slack glue | ~1 500 |
| `internal/config` | YAML config (`~/.config/camsnap/config.yaml`, XDG); load/save/upsert/find | 106 |
| `internal/rtsp` | `BuildURL(camera)`: builds the RTSP URL with escaped userinfo and default port/path | 63 |
| `internal/rtspclient` | gortsplib client: grabs the first H.264 IDR, then **pipes it to ffmpeg** to make a JPEG | 174 |
| `internal/exec` | Runs the ffmpeg subprocess, rewrites version-specific flags (`-fps_mode`/`-vsync`, `-timeout`/`-stimeout`, …), `ClassifyError`, `HasBinary` | 199 |
| `internal/discovery` | ONVIF WS-Discovery (UDP multicast 239.255.255.250:3702) and `GetDeviceInformation` SOAP (WS-Security UsernameToken digest, falling back to HTTP Basic) | 326 |
| `internal/slack` | Slack Web API: resolve channel/user to an ID, open a DM, 3-step external upload | 412 |

### Where ffmpeg is used today (everything here must be replaced natively)

| Use site | ffmpeg invocation (semantic) | Native replacement (Rust) |
|---|---|---|
| `snap` (default "ffmpeg" client) | `-timeout 7s -rtsp_transport X -skip_frame nokey -i URL -ss 2 -frames:v 1 -flags2 +showall -q:v 2 out` | RTSP client → H.264/H.265/MJPEG keyframe decode → image encode (§6.5) |
| `snap` (gortsplib client) | Feeds Annex-B H.264 into `ffmpeg -f h264 -i pipe:0 -frames:v 1 out` | Same decoder and encoder pipeline, "first keyframe" mode |
| `clip` | `-rtsp_transport X -i URL -t D -c:v copy [-an | -c:a aac|<codec>] out.mp4` (`-y`/`-n`) | RTSP → depacketize → MP4/MOV mux (video passthrough) + audio passthrough or PCM conversion. **AAC encoding is dropped** (§6.6, §7 Q14) |
| `audio` / `clip --audio-only` | `… -vn -c:a copy out.wav` | RTSP audio → WAV/MP4/ADTS mux, copy or transcode (§6.6) |
| `watch` | `-vf "select='gt(scene,T)',metadata=print" -f null -`; parses `scene_score=` from stderr | Full-rate video decode + bit-compatible reimplementation of ffmpeg's scene score (§6.8) |
| `doctor --probe` | 1 s probe with `-analyzeduration/-probesize 1e6`, 3 attempts, error classification | RTSP DESCRIBE/SETUP/PLAY and receive ≥1 frame, 3 attempts, typed error classification (§6.10) |
| `doctor` | "ffmpeg found in PATH" check | Report built-in media engine capabilities |

---

## 2. Hard constraints

- **C1: No ffmpeg, anywhere.** No `Command::new("ffmpeg")`, no `ffmpeg-next`/`ffmpeg-sys`/`rsmpeg`/`gstreamer` crates, no `libav*` linkage. CI enforces this (§9).
- **C2: Self-contained binary.** At runtime the binary needs only the OS (libc and, on macOS, the system libc++). No external executables, shared codec libraries, config files or network downloads are needed for media work. C/C++ code may be **vendored and statically compiled** at build time (e.g. OpenH264 source via the `openh264` crate) because the shipped artifact is still one file. The only subprocess the app may ever start is the **user-supplied `watch --action` shell command**.
- **C3: Feature parity.** Every row of §3 is ported. Internal ffmpeg shims (flag rewriting, binary lookup) are obsolete because no ffmpeg is used. §3 marks them as such.
- **C4: Config compatibility.** Existing `config.yaml` files written by Go camsnap must load unchanged, including `rtsp_client: ffmpeg|gortsplib`. Files written by Rust must stay loadable by Go camsnap (same keys).
- **C5: CLI compatibility.** Same command names, aliases, flag names, short flags, defaults, positional arguments, and the same stdout/stderr messages (§6). Scripts that call camsnap today must keep working.
- **C6: Platforms.** Release targets: `x86_64/aarch64-apple-darwin`, `x86_64/aarch64-unknown-linux-musl` (static), `armv7-unknown-linux-musleabihf` (the Makefile builds linux-arm v7 today), and `x86_64/aarch64-pc-windows-msvc`.
- **C7: No OpenSSL.** TLS uses `rustls` everywhere (HTTPS for Slack and ONVIF, TLS for RTSPS). Bundle root certificates (`webpki-roots`) so the binary stays self-contained.

---

## 3. Feature inventory / traceability matrix

Copy this table to `docs/port-status.md` and tick rows as you finish them. "Phase" refers to §8.

### 3.1 Global / CLI surface
| # | Feature | Phase |
|---|---|---|
| G1 | Root command `camsnap` with Short text, colorized Long text, Example block (§6.1) | 3 |
| G2 | `--version` flag on root prints `<version>\n` | 3 |
| G3 | `version` subcommand prints `<version>\n` | 3 |
| G4 | Global persistent `--config <path>`, valid before or after the subcommand | 3 |
| G5 | `-h/--help` on every command; `camsnap help <cmd>` | 3 |
| G6 | Shell completion subcommand (`camsnap completion bash|zsh|fish|powershell`). Cobra adds this automatically today | 3 |
| G7 | On error: print the error message to stderr, exit code 1. Do not print usage on runtime errors (`SilenceUsage`) | 3 |
| G8 | Version embedded at build time and overridable (Go used `-ldflags -X main.version`) | 1 |
| G9 | Colored output only when stdout is a TTY. Colors #4caf50 (ok), #ff9800 (warn), #e53935 (err), #00acc1 (accent) | 2 |
| G10 | Go duration syntax for all duration flags (`500ms`, `1.5s`, `1m30s`, `2h`, `0`) | 2 |
| G11 | Bool flags accept `--flag`, `--flag=true`, `--flag=false` (needed for `clip --overwrite=false`) | 3 |
| G12 | Extra positional args are silently ignored where Cobra ignored them (all commands except `slack test`) | 3 |

### 3.2 Config
| # | Feature | Phase |
|---|---|---|
| CF1 | Default path `$XDG_CONFIG_HOME/camsnap/config.yaml`, otherwise `~/.config/camsnap/config.yaml` (also on macOS and Windows) | 2 |
| CF2 | Missing file → empty config, no error | 2 |
| CF3 | Save creates parent dirs (0755) and writes the file with mode 0600 | 2 |
| CF4 | Camera fields: name, host, port, protocol, username, password (always emitted); path, rtsp_transport, stream, rtsp_client, no_audio, audio_codec (omitted when empty/false) | 2 |
| CF5 | `slack: {token, default_channel}` (omitted when empty) | 2 |
| CF6 | Upsert by exact name (whole entry replaced); find by exact name | 2 |
| CF7 | Unknown YAML keys are ignored on load | 2 |

### 3.3 Commands
| # | Feature | Phase |
|---|---|---|
| A1 | `add` with all flags; port defaults to 554 (0 → 554); protocol defaults to rtsp; prints Added/Updated | 3 |
| L1 | `list`: sorted by name, password masked, format `%-12s host=… port=… proto=<lower> auth=…`; warning when empty | 3 |
| S1 | `snap`: positional or `--camera`; `--out`, falling back to a temp `camsnap-*.jpg` with a message | 7 |
| S2 | `snap` standard mode (legacy name `ffmpeg`): first keyframe at ≥2 s stream time, high-quality encode, 7 s socket timeout | 7 |
| S3 | `snap` first-keyframe mode (legacy name `gortsplib`): first random-access frame, SPS/PPS prepended, 15 s r/w timeout | 7 |
| S4 | `snap` output format chosen by extension (jpg/jpeg default; png, bmp, tiff, webp, ppm) | 7 |
| S5 | `snap --timeout` overall deadline (default 30 s) | 7 |
| S6 | `--path` / `--stream` / per-camera defaults / conflict error (snap, clip, audio, watch) | 2, 7 |
| S7 | `--rtsp-transport tcp|udp` (snap, clip, audio, watch, doctor) | 6 |
| S8 | `--rtsp-auth auto|basic|digest` validation (snap, clip, audio, watch, doctor) | 2 |
| S9 | RTSPS (`protocol: rtsps`) with ffmpeg-like lenient certificate handling | 6 |
| S10 | Slack upload after a successful snap (flags, env, config default channel) | 4, 7 |
| C1 | `clip`: `--dur` (default 10 s) or `-s/--seconds`, mutually exclusive, >0 | 8 |
| C2 | `clip` video stream copy (H.264, H.265, MJPEG) into MP4/MOV | 8 |
| C3 | `clip` audio: default `auto` (copy when the container can hold the source codec, otherwise video-only with a note; replaces the old AAC transcode, see Q14); `--audio-codec <x>` (legacy `aac` accepted); `--no-audio`; per-camera `no_audio` / `audio_codec` | 8 |
| C4 | `clip --audio-only`: drop video, default codec copy, default temp extension `.wav`, overwrite defaults to false | 8 |
| C5 | Overwrite semantics: `clip` defaults to overwrite unless `--overwrite` was explicitly given or `--audio-only` is set; `audio` defaults to no overwrite | 8 |
| C6 | Temp output when `--out` is omitted (`camsnap-*.mp4` / `camsnap-*.wav`) with message | 8 |
| C7 | Slack upload after a successful clip/audio | 8 |
| C8 | Ctrl-C during recording finalizes a playable file (ffmpeg did this) | 11 |
| AU1 | `audio [camera]` (alias `record-audio`) = record with audio-only, own flag set and defaults | 8 |
| W1 | `watch`: positional or `--camera`; `--action` required; `--threshold` in (0,1), default 0.2 | 9 |
| W2 | Scene-change score identical to ffmpeg `select=gt(scene,T)` | 9 |
| W3 | `--cooldown` (default 5 s), first event always fires | 9 |
| W4 | `--duration` max runtime (0 = until interrupted); running actions are killed at the end | 9 |
| W5 | Output lines: key/value, or JSON with `--json` (exact formats §6.8) | 9 |
| W6 | Action run via shell with `CAMSNAP_CAMERA`, `CAMSNAP_SCORE`, `CAMSNAP_TIME`; fire-and-forget | 9 |
| W7 | `--action-template` with `{camera}`, `{score}`, `{time}` placeholders | 9 |
| W8 | When the stream ends unexpectedly: error with classification (auth / network-refused / network-timeout / not-found / unknown) | 9 |
| D1 | `discover`: WS-Discovery probe, dedupe, print host and the suggested `add` command; `--timeout` (default 3 s) | 5 |
| D2 | `discover --info`: GetDeviceInformation with WS-Security digest, then Basic/anonymous fallback; uses creds from config for a matching host | 5 |
| DR1 | `doctor`: media engine status, config path, TCP dial per camera (`--timeout` default 2 s) | 10 |
| DR2 | `doctor --probe`: RTSP probe per camera, 3 attempts 500 ms apart, timeout = dial timeout + 2 s, classification | 10 |
| SL1 | `slack set --token --channel`: stores into config | 4 |
| SL2 | `slack test <file> [--channel] [--message]`: default message "Test upload from camsnap" | 4 |
| SL3 | Channel resolution: `#name`, `name`, IDs (`C…`, `G…`, `D…`), `@user`, `user`, `U…` (opens a DM); pagination | 4 |
| SL4 | Token precedence: `--slack-token` > `SLACK_TOKEN` env > config. Channel precedence: flag > config default | 4 |
| SL5 | 3-step upload: `files.getUploadURLExternal` → raw POST → `files.completeUploadExternal` | 4 |

### 3.4 Internal Go items and their fate
| Go item | Fate |
|---|---|
| `exec.RunFFmpeg`, `RunFFmpegWithOutput`, `adaptFFmpegArgs`, `extractUnrecognizedOption`, `replaceOptionPair`, `removeOptionPair` | **Obsolete.** They exist only to handle ffmpeg CLI version differences, which can't happen without ffmpeg. Not ported. |
| `exec.HasBinary` | **Obsolete.** Its only use is the ffmpeg lookup. Not ported. |
| `exec.WithTimeout` | Replaced by `tokio::time::timeout` / deadline helpers (0 or negative = no timeout). Same semantics. |
| `exec.ClassifyError(string)` | **Ported** as `errors::classify_message(&str)` plus `errors::classify(&CamsnapError)` for typed errors (§6.11). |
| `rtspclient.GrabFrameViaGort`, `findH264` | **Ported** into `media::snapshot` (FirstKeyframe mode) and `media::session::find_video_stream`. |
| `cli.parseRTSPAuth`, `transportFlag`, `appendStream`, `appendPath`, `parseSceneScore`, `applyTemplate`, `safeName`, `findCreds`, `dialOnce`, styler | **Ported** 1:1 with their unit tests. `parseSceneScore` stays as a helper for the test suite and for parsing legacy log lines; the live path calls the scene detector directly. |

---

## 4. Target architecture

### 4.1 Crate layout (single package, lib + bin)

```
Cargo.toml
rust-toolchain.toml            # pin stable toolchain (>= 1.85, edition 2024)
src/
  main.rs                      # tiny: build runtime, call cli::run(), map errors → exit code
  lib.rs                       # pub mod declarations; enables integration tests
  version.rs                   # VERSION = option_env!("CAMSNAP_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
  cli/
    mod.rs                     # clap root struct, global --config, dispatch, error → exit code
    add.rs  list.rs  snap.rs  record.rs (clip + audio)  watch.rs
    doctor.rs  discover.rs  slack_cmd.rs  completion.rs
    style.rs                   # Styler (ok/warn/err/accent), TTY + color-profile detection
    flags.rs                   # RtspAuth, Transport, ClientMode enums; go-duration parser; bool parser
    resolve.rs                 # per-camera defaults merge; final URL; append_stream/append_path
  config.rs
  rtsp_url.rs                  # build_url (Go BuildURL); userinfo escaping identical to Go
  discovery/{mod.rs, info.rs}
  slack.rs
  timefmt.rs                   # Go-compatible RFC3339Nano; Go-compatible %q quoting
  errors.rs                    # CamsnapError (thiserror), classify(), classify_message()
  media/
    mod.rs
    session.rs                 # RTSP session wrapper (retina): creds, transport, TLS, timeouts, teardown
    codec.rs                   # VideoCodec / AudioCodec enums + parameters (SPS/PPS/VPS, ASC, rates)
    annexb.rs                  # AVCC↔Annex-B conversion, NAL helpers, keyframe detection
    video_decode.rs            # trait VideoDecoder + H264 (openh264), H265 (Phase-0 choice), MJPEG
    yuv.rs                     # YUV420 frame type, YUV→RGB (BT.601 limited / full, matching swscale defaults)
    image_out.rs               # encode by extension (jpeg q=95, png, bmp, tiff, webp-lossless, ppm)
    snapshot.rs                # Standard + FirstKeyframe pipelines
    recorder.rs                # clip/audio pipeline, start sync, duration, stop, finalize
    audio/{g711.rs, g726.rs, l16.rs, aac_dec.rs, transcode.rs}   # no AAC encoder (Q14)
    mux/{mp4.rs, wav.rs, adts.rs, mod.rs}   # container chosen by output extension
    scene.rs                   # ffmpeg-compatible scene score
    probe.rs                   # doctor probe
tests/
  support/{mod.rs, fake_rtsp.rs, fixtures.rs, slack_mock.rs, onvif_mock.rs}
  cli_*.rs                     # assert_cmd end-to-end tests (spawn the real binary)
docs/
  rust-decisions.md            # ADR log (Phase 0 output)
  port-status.md               # tick list (copy of §3)
```

### 4.2 Runtime model
- `tokio` multi-thread runtime, built in `main.rs`. All network I/O is async.
- CPU-heavy work (video decoding, scene scoring, audio decoding/PCM conversion, image encoding) runs on a **dedicated blocking thread** fed through a **bounded** channel (capacity ~8 frames). When the channel is full, the reader awaits (backpressure). This copies ffmpeg's single-pipeline behavior and keeps memory bounded.
- Cancellation goes through a `tokio_util::sync::CancellationToken`, fired on Ctrl-C, deadline expiry or `--duration` expiry.

### 4.3 Dependency selection (pin exact versions in `Cargo.toml`; verify each in Phase 0)

| Concern | Crate (preferred) | Notes |
|---|---|---|
| CLI | `clap` 4 (derive, `env`, `wrap_help`), `clap_complete` | |
| Async | `tokio` (rt-multi-thread, macros, net, time, signal, process, fs, io-util, sync), `tokio-util` | |
| RTSP | `retina` (Scott Lamb; pure Rust; TCP and UDP; Basic/Digest; H.264, AAC, simple audio, …) | Phase 0 checks: RTSPS, H.265, MJPEG, G.726, Opus. Fork or vendor if something is missing (§8 Phase 0) |
| H.264 decode | `openh264` with source build (vendored Cisco source, BSD-2, compiled statically) | Put it behind the `VideoDecoder` trait so a production-grade pure-Rust decoder can replace it later |
| H.265 decode | **Phase 0 decision** (candidates: vendored `libde265` via a small in-tree `-sys` crate built with `cc`, or a mature pure-Rust HEVC decoder if one exists) | Mind licensing (libde265 is LGPL-3). Record the choice in the ADR |
| MJPEG | `zune-jpeg` or `jpeg-decoder` (decode), or pure passthrough when the output is `.jpg` | |
| Image encode | `image` (features: jpeg, png, bmp, tiff, webp, pnm) | |
| AAC decode | `symphonia` (feature `aac`, pure Rust) | For AAC→PCM (audio-only to WAV, §7 Q6). AAC **passthrough** needs no codec at all (mux only) |
| AAC encode | **none: dropped by decision (§7 Q14)** | Do not add any AAC encoder dependency |
| G.711 / G.726 / L16 | in-tree (`media/audio/*`) | Small, table-driven, unit-tested against ITU vectors |
| MP4 mux | in-tree `media/mux/mp4.rs` (reference: retina's `examples/client/mp4.rs`, MIT/Apache) | Must write `avc1/avcC`, `hvc1/hvcC`, `mp4a/esds`, `Opus/dOps`, `jpeg` (mov), `alaw`/`ulaw` (mov) |
| YAML | `serde` + a maintained `serde_yaml` fork (e.g. `serde_yaml_ng`) | |
| JSON | `serde_json` | |
| HTTP | `reqwest` (default-features off; `rustls-tls-webpki-roots`, `json`, `stream`) | Slack + ONVIF |
| XML | `quick-xml` | Match on local names (namespace-agnostic), as Go's `encoding/xml` does |
| Crypto/IDs | `sha1`, `base64`, `rand`, `uuid` (v4) | WS-Security digest, nonces |
| Time | `chrono` (local tz) | Go-compatible RFC3339Nano helper in `timefmt.rs` |
| MIME | `mime_guess` | Slack upload content type |
| Temp files | `tempfile` (`Builder::prefix("camsnap-").suffix(".jpg").tempfile()?.keep()`) | |
| Colors | `std::io::IsTerminal`, `anstyle`, `ansi_colours` (RGB → nearest 256) | |
| Errors | `thiserror` (lib), `anyhow` (bin glue) | |
| URL | `url` | Careful: see the `hostPort` note in §6.9 |
| Dev | `assert_cmd`, `predicates`, `tempfile`, `insta` (snapshots of help text), `mp4` (reader, for asserting output), `hound` (WAV reader) | |

---

## 5. Data model (must stay byte-compatible in meaning)

```yaml
cameras:
  - name: kitchen          # always emitted
    host: 192.168.0.175    # always emitted
    port: 554              # always emitted (int)
    protocol: rtsp         # always emitted ("" allowed in old files)
    username: tapo         # always emitted ("" allowed)
    password: secret       # always emitted ("" allowed)
    path: Bfy47SNWz9n2WRrw # omit if empty
    rtsp_transport: udp    # omit if empty   (tcp|udp)
    stream: stream2        # omit if empty
    rtsp_client: gortsplib # omit if empty   (ffmpeg|gortsplib; also accept new aliases, §6.4)
    no_audio: true         # omit if false
    audio_codec: aac       # omit if empty
slack:                     # omit whole block if both fields empty
  token: xoxb-...
  default_channel: "#alerts"
```

- `cameras:` is always emitted. Write `cameras: []` when there are none.
- Loading tolerates a missing `cameras`, a `null` value, missing optional fields and unknown keys.
- Write atomically: write a temp file in the same dir with mode 0600, then rename. On Unix, set the mode with `OpenOptionsExt::mode(0o600)` **before** writing so secrets never sit in a world-readable file. Directory mode is 0755.

---

## 6. Behavioral specification (per command)

Notation: messages in `code` are printed exactly. `%q` means **Go-style quoting** (implement `timefmt::go_quote`, which matches `strconv.Quote`: wrap in `"`, escape `\` and `"`, use `\n` `\t` `\r` `\a` `\b` `\f` `\v`, `\xNN` for bytes < 0x20 and invalid UTF-8, keep printable Unicode, `\uNNNN`/`\UNNNNNNNN` for non-printable runes). Normal output goes to **stdout** (Cobra's `cmd.Printf` writes to stdout). Errors go to **stderr** through `main`.

### 6.1 Root
- Short: `camsnap captures frames and clips from RTSP cameras (Tapo first, Ubiquiti next)`
- Long (colorized when stdout is a TTY; accent = #00acc1, green = #4caf50, red = #e53935):
  ```
  camsnap – capture frames/clips and motion from RTSP/ONVIF cameras.

  Common commands:
    snap     grab a frame (positional camera name allowed)
    clip     short clip; drop audio with --no-audio
    audio    record audio clip (default 10s, copy codec, .wav)
    doctor   checks media engine, reachability, optional RTSP probe
  ```
  (`camsnap` and `Common commands:` accent; `snap`, `clip`, `audio` green; `doctor` red. The doctor line's wording changes from "checks ffmpeg" because ffmpeg is gone. This is the only text change.)
- Examples (verbatim; keep in this order):
  ```
    camsnap add --name kitchen --host 192.168.0.175 --user tapo --pass secret --rtsp-transport udp --stream stream2
    camsnap snap kitchen --out shot.jpg
    camsnap snap kitchen --slack-channel #security --slack-message "Kitchen check!"
    camsnap clip kitchen --dur 5s --no-audio --out clip.mp4
    camsnap clip kitchen --dur 10s --slack-channel @steve
    camsnap audio kitchen --seconds 10 --out audio.wav
    camsnap watch kitchen --threshold 0.2 --cooldown 5s --action 'camsnap snap kitchen --slack-channel #alerts'
    camsnap doctor --probe --rtsp-transport udp
  ```
- Global flag: `--config <path>`. Help: `Path to config file (default: $XDG_CONFIG_HOME/camsnap/config.yaml)`.
- Exit codes: 0 on success and for `--help`/`--version`. 1 for any error, including clap parse errors (clap uses 2 by default; override it). 130 after a Ctrl-C-finalized recording/watch.
- `--version` → `<version>\n`.
- Subcommands, in this order: add, list, snap, clip, audio, discover, watch, doctor, slack, version, completion, help.

### 6.2 `add` — "Add or update a camera"
Flags: `--name` (""), `--host` (""), `--port` (554, "Camera port (default 554)"), `--protocol` ("rtsp", "Protocol (rtsp or rtsps)"), `--user`, `--pass`, `--path` ("Explicit RTSP path (e.g., /Bfy... token from UniFi Protect)"), `--rtsp-transport` ("Preferred RTSP transport for this camera (tcp|udp)"), `--stream` ("Default RTSP stream path (stream1 or stream2)"), `--rtsp-client` ("Default RTSP client (ffmpeg|gortsplib)"), `--no-audio` (bool), `--audio-codec`.
1. If name or host is empty → error `name and host are required`.
2. port 0 → 554; protocol "" → "rtsp". Values are stored as given (no validation of transport/client strings, same as Go).
3. Load config (from `--config` or the default path), upsert, save.
4. Print `Added camera %q` (new) or `Updated camera %q` (replaced).

### 6.3 `list` — "List saved cameras"
- Empty → print warn-styled `No cameras saved. Add one with: camsnap add --name cam1 --host 192.168.1.50 --user tapo --pass secret`.
- Otherwise sort by name (byte order) and print per camera: `format!("{:<12} host={} port={} proto={} auth={}", name, host, port, protocol.to_lowercase(), auth)`, where auth is `username`, plus `:***` when both username and password are non-empty.

### 6.4 Shared capture options & resolution (snap / clip / audio / watch)
Enums (in `cli/flags.rs`):
- `RtspAuth`: parse case-insensitively. `""|auto → Auto`, `basic`, `digest`; anything else → error `invalid --rtsp-auth (use auto|basic|digest)`.
- `Transport`: `"" → Tcp`; `tcp`, `udp` (case-insensitive); otherwise → `invalid --rtsp-transport (use tcp|udp)`.
- `ClientMode`: `ffmpeg|standard|"" → Standard`; `gortsplib|fast → FirstKeyframe`. Unknown values fall back to Standard, because Go treated anything other than `gortsplib` as the ffmpeg path.

**Precedence (see §7 Q1):** explicit CLI flag > per-camera config value > built-in default. All such flags are `Option<T>` in clap so "explicitly given" can be detected.

**URL resolution** (`resolve.rs`). Replaces `BuildURL` + `appendPath`/`appendStream`:
1. If both `--stream` and `--path` are given → error `use --path for custom RTSP token URLs; omit --stream`.
2. `path = flag_path.or(cam.path)`. If a path exists, the stream is ignored.
3. `stream = flag_stream.or(cam.stream)` (only when there is no path).
4. Base = `rtsp_url::build_url(cam with path=path-or-None)`:
   - host "" → error `host is required`.
   - If host contains no `:`, append `:<port>` (port 0 → 554). If it contains `:`, use it verbatim.
   - Userinfo = `escape(user)` + (if password non-empty) `:` + `escape(pass)`, then `@`. Go's `encodeUserPassword` escaping: keep `A-Z a-z 0-9 - . _ ~ $ & + , ; =`; percent-encode everything else with uppercase hex (including `: @ / ?`, space, `!*'()%#[]`, non-ASCII bytes).
   - protocol "" → `rtsp`. Accept `rtsp`/`rtsps` case-insensitively but **keep the original case** in the output. Anything else → error `unsupported protocol %q`.
   - path "" → `/stream1`. Ensure a leading `/`.
5. If no path was given but a stream was, replace the last path segment with `/<stream>` (equivalent to Go `appendStream`).
6. Also port `append_stream(base, s)` and `append_path(base, p)` exactly as in Go (replace everything after the last `/`, add a leading `/` if missing, empty input → base unchanged) together with their unit tests.

**Validation order.** Keep the Go order so the same input gives the same first error:
- snap: camera name → temp-file creation (if `--out` is missing) → stream/path conflict → load config → find camera (`camera %q not found`) → path merge → build URL → transport/client defaults → `--rtsp-auth` → `--rtsp-transport` → capture.
- record (clip/audio): camera name → `--dur`/`--seconds` → `--audio-only`+`--no-audio` → out-path handling → load config → find camera → stream/path conflict → merges → `--rtsp-auth` → `--rtsp-transport` → build URL → capture.
- watch: camera name → `--action` → threshold → `--rtsp-auth` → `--rtsp-transport` → load config → find camera → stream/path conflict → merges → build URL → template → run.

The missing-camera error message is `--camera is required` on all four commands.

`--rtsp-auth` is passed to the session layer (see §6.12): Auto = respond to whatever challenge the server sends. Basic/Digest = only answer that scheme and fail with an auth error otherwise. If the chosen RTSP library can't restrict the scheme, validating only is acceptable (that is all Go did). Record it in the ADR.

### 6.5 `snap` — "Capture a single frame to a file"
Flags: `--camera`, `--out`, `--timeout` (30s, "Timeout for capture"), `--rtsp-auth` (auto), `--rtsp-transport` (Option; default tcp), `--stream`, `--path`, `--rtsp-client` (Option; default standard; help `RTSP client mode: ffmpeg|gortsplib (legacy names, both built-in)`), Slack flags (§6.13).
- No `--out`: create a persistent temp file `camsnap-*.jpg` in the OS temp dir, close it, and print `No --out provided, writing snapshot to <path>`. The path must be the **last whitespace-separated token** of that line, because tests and scripts parse it.
- Output format comes from the extension (case-insensitive): `.jpg/.jpeg` → JPEG quality 95 (≈ ffmpeg `-q:v 2`), 4:2:0 subsampling; `.png`; `.bmp`; `.tif/.tiff`; `.webp` (lossless); `.ppm`. Any other or missing extension → error `unsupported output format "<ext>" (use .jpg, .png, .bmp, .tiff, .webp or .ppm)`. Write to `<out>.partial` and rename it over `<out>` (overwrite always, like `-y`).
- **Standard mode** (emulates `-skip_frame nokey -ss 2 -flags2 +showall -frames:v 1`):
  - Socket/IO timeout 7 s per operation. Overall deadline `--timeout` (0 = none).
  - Use the first video stream (H.264, H.265 or MJPEG; prefer H.264 when there are several).
  - `t0` = timestamp of the first received video frame. Decode **only random-access frames** (IDR / CRA / BLA, an I-frame with recovery-point SEI, or any MJPEG frame). Keep feeding parameter sets.
  - Pick the first decoded keyframe with `pts − t0 ≥ 2.0 s`. If the decoder reports concealment or partial output, accept it anyway (`+showall`).
  - Convert YUV→RGB with BT.601 limited range unless the VUI says full range (`yuvj`), which matches swscale defaults. Encode and write.
- **FirstKeyframe mode** (emulates the gortsplib path): r/w timeout 15 s. Overall deadline `--timeout`. Take the **first** random-access access unit, prepend SPS/PPS (and VPS for H.265) from the SDP when the frame doesn't carry them in-band, decode, encode, write. The Go version only supported H.264 here; Rust supports every supported codec (a superset).
- On success, run the Slack step (§6.13).
- Errors: `timeout waiting for frame`, `no supported video track found (found: <codecs>)`, plus session errors (§6.11).

### 6.6 `clip` — "Record a short clip to a file" and `audio [camera]` — "Record an audio-only clip to a file" (alias `record-audio`)
`clip` flags: `--camera`, `--out` ("Output file (e.g., clip.mp4)"), `--dur` (10s), `-s/--seconds` (int), `--timeout` (30s), `--rtsp-auth`, `--rtsp-transport`, `--stream`, `--path`, `--no-audio`, `--audio-only`, `--audio-codec` ("Audio codec (default auto for video clip, copy for audio-only); ignored if --no-audio"), `--overwrite` (bool, accepts `=false`), Slack flags.
`audio` flags: the same minus `--no-audio`/`--audio-only`. `--out` help is "Output file (e.g., output_audio.wav)", `--audio-codec` help is "Audio codec (default copy)", `--dur` help is "Recording duration (e.g., 10s)". `audio` always means audio_only=true and no_audio=false.

Effective overwrite:
- `clip`: if `--overwrite` was **explicitly given**, use its value. Otherwise use `true`, unless `--audio-only` is set, in which case use `false`.
- `audio`: the `--overwrite` value (default false).

Steps (shared `record::execute`):
1. Camera name (positional or `--camera`), else `--camera is required`.
2. If both `--seconds` and `--dur` were explicitly given → `cannot specify both --dur and --seconds`. If `--seconds` was given and is ≤ 0 → `--seconds must be > 0`. If the effective duration is ≤ 0 → `--dur must be > 0`.
3. audio_only && no_audio → `cannot combine --audio-only and --no-audio`.
4. No `--out` → temp `camsnap-*.mp4` (label `clip`) or `camsnap-*.wav` (label `audio`). If overwrite is false, delete the placeholder. Print `No --out provided, writing <label> to <path>`.
   `--out` given, overwrite false and the file exists → `output file %q already exists (use --overwrite to replace)`. Leave the existing file untouched.
5. Load config, find camera, check stream/path conflict, merge path/stream/transport.
6. Audio defaults:
   - audio_only: no_audio := false. codec := flag, else cam.audio_codec, else `copy`.
   - Otherwise: no_audio := flag || cam.no_audio. codec := flag, else cam.audio_codec, else `auto` (see §7 Q14).
7. Validate auth and transport, build the URL, connect, record (below), finalize the file, then the Slack step.

**Container by extension** (case-insensitive): `.mp4`, `.m4v` → MP4 (ISO BMFF). `.mov` → QuickTime. `.m4a` → MP4 audio-only. `.wav` → WAV. `.aac` → ADTS. Anything else → error listing the supported extensions. Audio-only into `.mp4`/`.mov`/`.m4a` is allowed. Video into `.wav`/`.aac` → error `output format <ext> cannot hold video (use --audio-only)`.

**Supported `--audio-codec` values:** `auto`, `copy`, `aac` (legacy alias; see below), `pcm_s16le`, `pcm_alaw`, `pcm_mulaw`. Any other value → error `unsupported audio codec %q (supported: auto, copy, pcm_s16le, pcm_alaw, pcm_mulaw)`.

**`auto` and legacy `aac` semantics (§7 Q14 — no AAC encoder):**
- `auto`: if the container natively supports the source audio codec in copy mode (e.g., AAC/Opus into `.mp4`/`.mov`/`.m4a`, or PCMA/PCMU/L16 into `.mov`/`.wav`), copy the audio stream. If the source has no audio track, record video-only silently. If the source audio codec cannot be stored in the target container (e.g., Tapo PCMA/G.711 into `.mp4`), omit the audio track (record video-only) and print a stderr note: `note: <src_codec> audio cannot be stored in <ext> without transcoding; recording video only (use .mov to keep audio or --no-audio to silence this note)`.
- `aac`: accepted for backward compatibility with existing configs (`audio_codec: aac`) and CLI invocations (`--audio-codec aac`). If the camera already streams AAC, treat `aac` as `copy`. Otherwise, if `aac` came from `cam.audio_codec` in config, treat it as `auto` with a stderr warning (`note: AAC encoding is not supported; falling back to auto audio handling`). If `--audio-codec aac` was passed explicitly on the CLI for a non-AAC source, return an error: `AAC transcoding is not supported (source audio is <src_codec>; use --no-audio, output to .mov, or use camsnap audio for .wav)`.

**Input codecs that must be handled** (depacketize, and decode where PCM conversion is needed):
- Video (copy only): H.264 (RFC 6184), H.265 (RFC 7798), MJPEG (RFC 2435; MOV/MP4 `jpeg` sample entry).
- Audio: PCMA/PCMU (G.711), L16 (big-endian PCM), AAC (RFC 3640 `mpeg4-generic` and MP4A-LATM if retina supports it), G.726-16/24/32/40, Opus (copy only).

**Codec × container compatibility matrix** (for explicit `--audio-codec` values; `auto` uses the `copy` rows and falls back to video-only when `✖`):

| Source → requested | `.mp4/.m4a` | `.mov` | `.wav` | `.aac` |
|---|---|---|---|---|
| copy AAC | ✔ mp4a | ✔ | ✖ when copy was explicit (hint: use `.m4a` or `--audio-codec pcm_s16le`); see §7 Q6 for the implicit default | ✔ ADTS |
| copy Opus | ✔ Opus/dOps | ✔ | ✖ | ✖ |
| copy PCMA/PCMU | ✖ (hint: use `.mov` or `--no-audio`) | ✔ alaw/ulaw | ✔ fmt 6/7 | ✖ |
| copy L16 | ✖ | ✔ `twos` | ✔ fmt 1 (byte-swap to LE) | ✖ |
| copy G.726 | ✖ | ✖ | ✔ fmt 0x0045 | ✖ |
| pcm_s16le | ✖ | ✔ `sowt` | ✔ | ✖ |
| pcm_alaw/pcm_mulaw | ✖ | ✔ | ✔ | ✖ |

PCM conversion (when `pcm_s16le`, `pcm_alaw`, or `pcm_mulaw` is requested, or via §7 Q6): decode source to PCM s16 (G.711 tables, G.726 ITU decoder, L16 byte-swap, AAC via `symphonia`), then write PCM or G.711 table-encode as requested. No resampling is required. If the source has no audio track, record video only without an error (same as ffmpeg).

**Recording semantics** (emulating `-t D -c:v copy`):
- Overall deadline = `--timeout` **+ duration** (see §7 Q4). Per-operation IO timeout 10 s.
- With video: drop everything until the first video random-access frame (ffmpeg drops leading non-key frames during stream copy). That frame's PTS is `start`. Drop audio before `start`. Stop when a video frame with `pts − start ≥ D` arrives (it is not written). Write audio samples with `pts < start + D`.
- Audio-only: `start` = first audio frame PTS. Stop at `pts − start ≥ D`.
- MP4 writer: stream `mdat` to disk (64-bit size if needed) and write `moov` at the end (ffmpeg default; no faststart). Timescale 90 000 for video, the sample rate for audio. Sample durations come from consecutive PTS deltas; the last sample reuses the previous delta. Keyframes go into `stss`. If PTS goes non-monotonic (B-frames), write in arrival order with a monotonic DTS and emit `ctts`. Track `tkhd` width/height comes from the SPS. `avcC`/`hvcC` comes from the SDP or in-band parameters; if the parameters change mid-clip, use a new sample description index.
- WAV writer: write the header with placeholder sizes and patch it on finalize. Use `RF64`/fallback only if larger than 4 GiB (not expected).
- File creation: if overwrite is false, open with `create_new` (atomic no-clobber, equivalent to `-n`). Otherwise create or truncate. On error, remove partially written outputs **except** after Ctrl-C, where the file is finalized and kept (§3 C8).

### 6.7 Slack after capture
See §6.13. It runs only after the capture succeeded and the file is finalized.

### 6.8 `watch` — "Run motion detection and execute an action"
Flags: `--camera`, `--action` ("Command to execute when motion detected"), `--threshold` (0.2), `--cooldown` (5s), `--duration` (0, "Optional max runtime (0 = until interrupted)"), `--json`, `--action-template`, `--rtsp-auth`, `--rtsp-transport`, `--stream`, `--path`.
Errors: `--action is required (e.g., "say motion" or "touch /tmp/motion")` (the action is required even when `--action-template` is given; Go parity). `--threshold must be between 0 and 1 (e.g., 0.2)` when `t ≤ 0 || t ≥ 1`.

If a template is given, `action` is replaced at startup by the template rendered with score 0 and `now`. That value appears in the text log line.

**Pipeline:** connect (video only, the equivalent of `-an -sn -dn`), decode **every** frame, and compute the scene score per frame:

```
// ffmpeg libavfilter/vf_select.c get_scene_score(), 8-bit YUV planar
if prev exists and same width/height:
    sad = Σ over planes p∈{Y,U,V} of Σ |cur_p[x,y] − prev_p[x,y]| over plane dims
          (chroma dims = ceil(w/2) × ceil(h/2) for 4:2:0; use the real subsampling for 4:2:2/4:4:4)
    count = Σ plane pixel counts
    mafd = sad / count                      // f64
    diff = |mafd − prev_mafd|
    score = clamp(min(mafd, diff) / 100.0, 0, 1)
    prev_mafd = mafd
else score = 0 (and prev_mafd is unchanged)
prev = cur
```

`prev_mafd` starts at 0. A frame "fires" when `score > threshold` (strictly greater, like `gt`). Use SIMD-friendly loops (u8 abs-diff summed into u64). Unit-test the function against hand-computed vectors and against ffmpeg-produced reference values captured **once, offline** and stored as JSON in `tests/fixtures/scene_reference.json` (only numbers are stored; nothing runs ffmpeg at test time).

**On fire** (cooldown: `last.is_none() || now − last ≥ cooldown`):
- `--json`: `{"event":"motion","camera":"<camera>","score":<%.3f>,"time":"<RFC3339Nano>"}` (camera is inserted raw, not JSON-escaped; Go parity).
- Otherwise: `event=motion camera=<camera> score=<%.3f> action=<go_quote(action)> time=<RFC3339Nano>`.
- Command to run = template rendered with (camera, score, now) when a template is set, otherwise `action`.
- Spawn it without waiting: Unix `sh -c <cmd>`, Windows `cmd /C <cmd>`. Environment = **inherited environment** plus `CAMSNAP_SCORE=<%.3f>`, `CAMSNAP_TIME=<RFC3339Nano>`, `CAMSNAP_CAMERA=<camera>` (see §7 Q5). Spawn errors are ignored (best effort). Reap children in the background so they don't become zombies. Keep their handles so they can be killed when watch ends because of `--duration`/cancellation (Go's `CommandContext` behavior).

**RFC3339Nano** (`timefmt::rfc3339_nano`): local time, `YYYY-MM-DDTHH:MM:SS[.fraction]±HH:MM`, or `Z` when the offset is 0. The fraction has trailing zeros trimmed and is omitted entirely when 0 (Go `time.RFC3339Nano` semantics).

**Template** replaces `{camera}`, `{score}` (`%.3f`) and `{time}`. Every occurrence of each is replaced.

**Termination:** `--duration` elapsed or Ctrl-C → return Ok (Ctrl-C → exit 130). The stream ends or errors otherwise → `stream ended: <error> (<class>)`, where class comes from §6.11.

### 6.9 `discover` — "Discover cameras on the local network via ONVIF WS-Discovery"
Flags: `--timeout` (3s), `--info` ("Attempt ONVIF GetDeviceInformation (may require credentials)").
- Load the config, **ignoring errors** (Go parity).
- Probe: bind UDP4 `0.0.0.0:0` and send the exact probe XML (copy the template from `internal/discovery/discovery.go`, with `uuid:<random v4>` as MessageID) to `239.255.255.250:3702`. Read until the deadline (timeout ≤ 0 → 3 s) with an 8 KiB buffer. Ignore malformed packets. Read errors other than timeout → error `read udp: …`.
- Parse: the root local name must be `Envelope`. Collect `Body/ProbeMatches/ProbeMatch/XAddrs` text and split on whitespace. Zero addresses → parse error (packet ignored).
- `host_port(addr)`: return the **raw authority** of the URL (between `scheme://` and the next `/`, `?` or `#`, with userinfo stripped), keeping an explicit port even when it is the scheme default (Go `u.Host` semantics; the `url` crate would drop `:80`). On parse failure or an empty host → "".
- Dedupe by host, falling back to the address when the host is empty. Keep first-seen order.
- None found → warn `No devices found. Ensure cameras and this host are on the same LAN.`
- Per device: `<ok-styled host>\t(add: camsnap add --name cam-<safeName(host)> --host <host> --user <user> --pass <pass>)<info>`, where `safeName` = host up to the first `:`. `<info>` = ` [<parts joined by ", ">]` with parts `model`, `fw <firmware>`, `manufacturer` (only the non-empty ones). Omit it when empty or on any error.
- Credentials for `--info`: the first camera where `cam.host == host || host.starts_with(cam.host + ":")`.
- **GetDeviceInformation:** POST to the XAddr with `Content-Type: application/soap+xml; charset=utf-8` and a 5 s timeout **per request** (§7 Q3). With a user: send the WS-Security UsernameToken header (nonce = 16 random bytes; created = UTC RFC3339Nano; digest = base64(sha1(nonce ‖ created ‖ password)); the username XML-escaped), using the exact XML from `info.go`. If it fails with an auth error (401/403 → `auth failed (status N)`), retry with an empty header plus `Authorization: Basic …` (only when a user is set). Other failures are returned as-is. Non-200 → `device info status N: <first 256 bytes, trimmed>`. Parse `Body/GetDeviceInformationResponse/{Manufacturer,Model,FirmwareVersion,SerialNumber,HardwareId}` and trim each value.

### 6.10 `doctor` — "Run basic checks (media engine, config present, camera ports reachable)"
Flags: `--timeout` (2s, dial timeout per camera), `--probe` ("Probe each RTSP URL briefly"), `--rtsp-auth`, `--rtsp-transport` (default tcp, "(probe)").
1. Load config (an error aborts).
2. Print ok-styled `✔ built-in media engine (H.264, H.265, MJPEG decode; MP4/WAV mux; no ffmpeg required)`. Build this list from the compiled-in capabilities. It replaces the ffmpeg PATH line.
3. `Config file: <path>`.
4. No cameras → warn `No cameras saved. Add one with camsnap add ...`, then return Ok.
5. Per camera, in config order: addr = `host:port` (port 0 → 554; IPv6 hosts are bracketed like Go `JoinHostPort`). TCP connect with timeout (≤0 → 2 s).
   - Failure → `<err ✖> <name> dial <addr> failed: <error>` and continue.
   - Build URL failure → `<err ✖> <name> RTSP URL invalid: <error>` and continue.
   - With `--probe`: validate auth/transport (same errors as §6.4; reported as a probe failure), then up to 3 attempts with deadline = timeout + 2 s each and 500 ms sleep between them. One attempt = DESCRIBE → SETUP (first video stream, or else the first stream) → PLAY → receive ≥1 complete frame. Failure → `<err ✖> <name> probe failed: <last error> (<class>)` and continue.
   - Success → `<ok ✔> <name> reachable at <addr>`.
   - The probe uses the camera's base URL from `build_url` (no stream/path flags; Go parity) and the `--rtsp-transport` flag (default tcp).

### 6.11 Error classification (`errors.rs`)
- `classify_message(s)` is a 1:1 port of Go `ClassifyError`: lower-case, then check in order: contains `401`|`unauthorized`|`auth` → `auth`; `connection refused` → `network-refused`; `timed out`|`timeout` → `network-timeout`; `not found` → `not-found`; otherwise `unknown`. Port the Go table tests.
- `classify(&CamsnapError)`: typed mapping first (RTSP 401/403 or auth-scheme mismatch → auth; `io::ErrorKind::ConnectionRefused` → network-refused; timeouts / `TimedOut` → network-timeout; RTSP 404 / no such stream → not-found). Then fall back to `classify_message(err.to_string())`.

### 6.12 RTSP session layer (`media/session.rs`)
- Input: the final URL (with userinfo), transport, auth preference, IO timeout and a TLS policy.
- Parse with `url`, percent-**decode** user and password, strip the userinfo from the URL and pass the credentials to the RTSP library (retina rejects URLs that contain credentials).
- `rtsp://` → TCP. `rtsps://` → TLS via `tokio-rustls` with **certificate verification disabled by default**. That matches ffmpeg's default `tls_verify=0` and UniFi Protect's self-signed certs. Document it in the README security notes. (`build_url` always writes an explicit port, so there is no implicit-default-port logic to handle.)
- Transport TCP = interleaved. Transport UDP = RTP/RTCP over UDP (retina `Transport::Udp`).
- Enable every leniency option the library offers (initial timestamp policy permissive, tolerate missing/odd RTCP, ignore zero sequence numbers, unknown SSRC), because cameras such as Tapo are quirky.
- Wrap DESCRIBE/SETUP/PLAY and every `next()` in `tokio::time::timeout(io_timeout)`.
- **Teardown:** always send TEARDOWN on exit and wait up to 2 s (retina `SessionGroup::await_teardown` or equivalent). This matters for Tapo cameras, which allow very few concurrent sessions.
- User-Agent: `camsnap/<version>`.
- Output: an async stream of `MediaFrame { stream_idx, kind: Video{codec, is_rap, data_annexb_or_avcc, new_params} | Audio{codec, data}, pts: Duration/ticks, clock_rate }`.

### 6.13 Slack
Flags on snap/clip/audio: `--slack-channel` ("Slack channel or user to upload to (e.g. #general, @alice)"), `--slack-token` ("Slack API token (overrides config and SLACK_TOKEN env var)"), `--slack-message` ("Optional message to post with the file").
- token = flag (non-empty) → `SLACK_TOKEN` env (non-empty) → `config.slack.token`. channel = flag (non-empty) → `config.slack.default_channel`.
- If the channel is empty → skip silently. **Note:** a configured default channel means every snap/clip uploads. That is Go behavior; keep it.
- Token empty → error `slack-channel specified but no token found (use --slack-token or SLACK_TOKEN environment variable)`.
- Print `<accent →> uploading <file> to Slack (<channelArg>)…`, resolve (wrap errors with `resolving slack channel: `), upload (wrap with `uploading to slack: `), then print `<ok ✔> Slack upload complete! File ID: <id>`, or `<ok ✔> Slack upload complete!` when no files came back.
- **ResolveChannel(nameOrID)**, exactly:
  1. Starts with `@` → resolve the user ID of the remainder, then `conversations.open`.
  2. Strip one leading `#`.
  3. If `len > 1` and the first byte differs from its ASCII-lowercased form (i.e. an uppercase letter): IDs starting with `U` → open a DM; otherwise return as-is.
  4. If `is_slack_id` (9–11 chars, all `A-Z0-9`): `U…` → open a DM, else return it.
  5. Paginate `GET {base}/conversations.list?limit=200&exclude_archived=true&types=public_channel,private_channel[&cursor=<urlenc>]` and return the ID on an exact name match.
  6. Otherwise try the user lookup and open a DM.
  7. Otherwise error `channel or user %q not found (make sure the bot is a member)`.
- `resolve_user_id`: `is_slack_id && starts_with('U')` → itself. Otherwise paginate `GET {base}/users.list?limit=200[&cursor=]` and match `name == x || profile.display_name == x`. Not found → `user %q not found`.
- `open_dm`: `POST {base}/conversations.open`, JSON `{"users":"<id>"}` → `channel.id`.
- Every API call sends `Authorization: Bearer <token>`. If `ok:false` → `slack API error: <error>`.
- **UploadFile:** open and stat the file; name = basename; MIME via `mime_guess`, falling back to `application/octet-stream`.
  1. `GET {base}/files.getUploadURLExternal?filename=<urlenc>&length=<size>` → `upload_url`, `file_id` (wrap `getting upload URL (step 1/3): `).
  2. `POST upload_url` with the raw body streamed from the file, `Content-Type: <mime>` and `Content-Length`. Accept 200/201/204, otherwise `upload failed with status N: <body>` (wrap `uploading file content (step 2/3): `).
  3. `POST {base}/files.completeUploadExternal` with JSON `{"files":[{"id":…}],"channel_id":…?,"initial_comment":…?}` (fields omitted when empty; wrap `completing upload (step 3/3): `).
- The base URL is configurable on the client struct (the default is `https://slack.com/api`) so tests can point it at a mock server.
- HTTP timeouts: connect 10 s, request 5 min (Go had none, so this is strictly more robust).
- `slack set`: flags `--token`, `--channel`. Update only the non-empty values and save. Print `Slack configuration updated in <path>` (see §7 Q2 for the path).
- `slack test <file>` (exactly one arg; clap error otherwise): flags `--channel` ("Slack channel or user (overrides default)"), `--message` ("Message to post"). The token comes from env/config only. Errors: `no Slack token found (use SLACK_TOKEN env var or 'camsnap slack set --token')`, `no Slack channel specified (use --channel or 'camsnap slack set --channel')`. Default message `Test upload from camsnap`. Resolve (wrap `resolving channel: `), print `Uploading <file> to <channel> (<id>)…`, upload, then print `Success! File ID: <id>` or `Success!`.

### 6.14 Styling (`cli/style.rs`)
- Color is enabled when stdout is a terminal, `NO_COLOR` is unset and `TERM != dumb`. `CLICOLOR_FORCE=1` forces it on. Truecolor when `COLORTERM` ∈ {truecolor, 24bit}; otherwise map the RGB to the nearest xterm-256 color via `ansi_colours`.
- `Styler { ok, warn, err, accent }`, each a plain passthrough when color is disabled. Every colored string in §6 goes through it.

### 6.15 Go-duration parser (`cli/flags.rs`)
Implement Go's `time.ParseDuration`: optional sign, then one or more `<decimal number><unit>` groups with units `ns`, `us`, `µs`, `μs`, `ms`, `s`, `m`, `h`. A bare `0` is allowed. Fractions are allowed (`1.5s`). Empty input or a missing unit → error `invalid duration "<s>"` (clap shows it). Negative values parse (later validation rejects them where Go did). Represent the result as `i128` nanoseconds or a signed wrapper to keep negative values.

---

## 7. Known Go quirks & explicit decisions

Each item says whether the Rust port **preserves** the Go behavior or **fixes** it. Do not decide these differently without updating this table and the ADR.

| ID | Go behavior | Decision |
|---|---|---|
| Q1 | `--rtsp-transport` defaults to `"tcp"` and `--rtsp-client` to `"ffmpeg"`, so the check `if transport == "" && cam.RTSPTransport != ""` never runs and **per-camera `rtsp_transport`/`rtsp_client` are silently ignored**, despite README/CHANGELOG promising per-camera defaults. | **Fix** using the precedence flag > camera > default (§6.4). This is the documented feature. |
| Q2 | `slack set` without `--config` calls `config.Load("")`/`Save("")` and fails with `write config: open : no such file or directory`. | **Fix**: use the same path resolution as every other command. |
| Q3 | `discover --info` reuses the discovery context, which has already expired by the time info is fetched, so info is effectively never shown. | **Fix**: give each info request its own 5 s timeout. |
| Q4 | `clip --timeout` (30 s) is an absolute deadline, so a `--dur 60s` clip is killed and the file is corrupted unless the user raises `--timeout`. | **Fix**: deadline = timeout + duration. |
| Q5 | The watch action gets **only** the 3 CAMSNAP_* variables (`cmd.Env` replaced the whole environment, so there is no PATH or HOME). | **Fix**: inherit the environment and add the 3 variables. |
| Q6 | `audio` defaults to `copy` into `.wav`. For AAC sources ffmpeg writes a non-standard WAV (tag 0x00FF) that most players can't play. | **Improve**: when the codec was **not explicitly** set (defaulted to `copy`) and the source can't be stored natively in WAV (AAC, Opus), transcode to `pcm_s16le` and print to stderr `note: <codec> audio cannot be copied into WAV; converting to pcm_s16le`. An explicit `--audio-codec copy` → error with hints. |
| Q7 | A multi-segment `path` gets duplicated (`/a/b` → `/a/a/b`) by `appendPath` after `BuildURL`. | **Fix** by the single resolution in §6.4. Single-segment paths (UniFi tokens) behave identically. |
| Q8 | `--rtsp-auth` is validated but never used. | Pass it to the session layer when possible (§6.4); otherwise validate only. |
| Q9 | `watch` requires `--action` even when `--action-template` is given. | **Preserve.** |
| Q10 | The JSON line inserts the camera name unescaped. | **Preserve** (names come from your own config). |
| Q11 | `main.go` says version `0.2.0` while the CHANGELOG says `0.3.0`. | Rust starts at **`0.4.0`** (CHANGELOG entry: "Rewritten in Rust; ffmpeg no longer required"). |
| Q12 | On Windows, the Go watch action used `sh -c`, which fails without a POSIX shell. | **Fix**: use `cmd /C` on Windows. |
| Q13 | The snap `--timeout` help said "Timeout for ffmpeg invocation". | Help text becomes "Timeout for capture". Flag name and default stay the same. |
| Q14 | `clip` defaulted to `-c:a aac` (transcoding PCMA/G.711 camera audio into AAC for MP4). | **Drop AAC encoding.** Avoids bundling `fdk-aac` or an immature AAC encoder. `clip` defaults to `auto`: copy audio when the target container supports the source codec (e.g., AAC/Opus in `.mp4`, or PCMA/PCMU in `.mov`); otherwise record video-only into `.mp4` with a stderr note. Existing `audio_codec: aac` configs still work (copy when source is AAC, else fall back to `auto` with a note). AAC **passthrough** (`copy`) and AAC **decoding** (via pure-Rust `symphonia` for `.wav` output) remain supported. |

---

## 8. Phased implementation plan

> For each phase: implement, add or port tests, run `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`, and tick §3 rows. Commit at the end of each phase with message `rust-port: phase N – <title>`.

### Phase 0: Spikes & decisions (no product code)
Create `spikes/` (deleted in Phase 12) and `docs/rust-decisions.md`. Answer each item with evidence: a small runnable spike plus notes.
1. **retina capability audit.** Check against the latest release: RTSPS/TLS, UDP transport, H.265 depacketization, MJPEG (RFC 2435), AAC (mpeg4-generic and LATM), G.711, G.726, L16, Opus, credential handling, auth-scheme restriction, teardown API, leniency options.
   *Decision rule:* anything missing is implemented by (a) an upstream PR if small, or (b) a **vendored fork** at `vendor/retina` (Cargo `[patch]`), adding a generic `AsyncRead+AsyncWrite` stream for TLS and the missing depacketizers. Do **not** switch to an ffmpeg- or GStreamer-based client.
2. **H.264 decoder.** Confirm `openh264` source build works (static, all 7 targets including musl and armv7 via `cargo-zigbuild` or `cross`). Measure decode speed for 1080p@15 fps on an arm64 Pi-class machine. Watch needs real-time decoding. Record the result.
3. **H.265 decoder.** Evaluate the candidates (vendored libde265 via `cc`, pure-Rust options). Criteria: static build on all targets, correct output on conformance streams, license compatible with distribution (record it), real-time 1080p on arm64. Pick one and ship it enabled by default.
4. **Cross-compilation matrix.** A hello-world binary that links openh264 and the HEVC decoder builds for all C6 targets. Record the exact toolchain commands.
5. **Scene reference values.** Offline, once, on a developer machine that **does** have ffmpeg: generate `tests/fixtures/scene_reference.json` (per-frame scores for a synthetic test clip whose generator is also committed, §8 Phase 6). Store only the numbers. This is the single sanctioned use of ffmpeg in the whole project, it happens outside the codebase, and the result is static data.

**Acceptance:** `docs/rust-decisions.md` holds one ADR per item (context, options, decision, consequences, license notes), and every spike compiles.

### Phase 1: Scaffold, toolchain, CI
- `cargo init --lib` at the repo root (next to the Go code; Go ignores `src/` and Cargo ignores `internal/`). Add `src/main.rs`, `rust-toolchain.toml`, `Cargo.toml` with `[profile.release] lto = "thin", codegen-units = 1, strip = true, panic = "abort"`.
- `version.rs` reads `CAMSNAP_VERSION` at build time and falls back to `CARGO_PKG_VERSION` (G8).
- `.gitignore`: add `/target`.
- New CI workflow `.github/workflows/rust.yml` (keep the Go CI until Phase 12): fmt, clippy `-D warnings`, test on ubuntu/macos/windows, the **no-ffmpeg guard** (§9), and a build of all release targets.
- **Acceptance:** CI green with an empty CLI (`camsnap --version` prints `0.4.0`).

### Phase 2: Pure logic modules + ported unit tests
Implement `config.rs`, `rtsp_url.rs`, `cli/flags.rs` (enums, duration and bool parsers), `cli/resolve.rs`, `timefmt.rs`, `errors.rs`, `cli/style.rs`.
Port **every** Go unit test with the same cases:
- config: `SaveLoadRoundTrip`, `DefaultConfigPathXDG`, `LoadMissingOK`, `UpsertCamera`, `FindCamera`, `SavePermissions` (Unix only). Plus new tests: loading a Go-written fixture YAML (4-space indent), unknown keys, `cameras: null`.
- rtsp url: `BuildURL`, `BuildURLInvalidProtocol`, `BuildURLDefaults`, `BuildURLWithPath`. Plus escaping cases (`p@ss:w/rd?` → `p%40ss%3Aw%2Frd%3F`, `a$b&c` stays the same, space → `%20`).
- helpers: `AppendStream`, `AppendPath`, `ParseRTSPAuth`, `TransportFlag`.
- watch helpers: `ParseSceneScore` and `applyTemplate`.
- exec: `ClassifyError` table and `WithTimeoutZero` (deadline helper with 0 = none).
- New: go-duration parser table (`0`, `10ms`, `1.5s`, `1m30s`, `-1s`, `2h45m`, errors: ``, `10`, `1x`); `go_quote` table; `rfc3339_nano` (trimming, `Z` vs offset); resolution precedence matrix (flag/camera/default × transport/client/stream/path).

Design for testability: functions take explicit inputs (e.g. `default_config_path(env: &impl Env)`) so tests never mutate process env. Rust 2024 makes `set_var` unsafe.
**Acceptance:** all ported tests pass. Rows CF1–CF7, G9, G10, S8 and the logic half of S6 are ticked.

### Phase 3: CLI skeleton (all commands, all flags, help) + simple commands
- Build the full clap tree with **every** flag, default, short flag, alias and help string from §6. Capture commands are stubs that return `not implemented` for now.
- Implement fully: `add`, `list`, `version`, `--version`, `completion <shell>` (`clap_complete`), `help`, and global `--config`.
- Bool flags: `ArgAction::Set` + `num_args(0..=1)` + `default_missing_value("true")` + `require_equals(true)` (G11). Detect "explicitly given" with `Option<bool>` / `ArgMatches::value_source`.
- Allow ignored trailing positionals (G12) with a hidden `trailing_var_arg` `Vec<String>`.
- Error rendering: `main` prints `{err}` (anyhow chain joined with `: `) to stderr and exits 1. Clap errors also exit 1, except help/version → 0.
- Tests (assert_cmd, spawning the binary with `XDG_CONFIG_HOME` set per test): `AddAndList`, `RootVersionHelp`, `RootVersionFlag` (`--version` prints `test-version`; build the test binary with `CAMSNAP_VERSION=test-version`, or check `CARGO_PKG_VERSION`), `add` update message, `list` masking & sort, `insta` snapshots of `--help` for every command.
- **Acceptance:** G1–G8, G11, G12, A1 and L1 ticked. The help snapshots have been reviewed against the Go `--help` output (run the Go binary manually and diff the flags and defaults).

### Phase 4: Slack
- Implement `slack.rs` and `cli/slack_cmd.rs` (§6.13) plus the `maybe_upload` helper.
- Tests: port `IsSlackID`, `ResolveChannel` (the same 7 cases against a mock HTTP server in `tests/support/slack_mock.rs`; a tiny `tokio` + `hyper` server or `wiremock`), `ResolveSlackToken`, `ResolveSlackChannel`. New: pagination (cursor followed), the full 3-step upload against the mock (assert headers, query, body bytes, JSON shape with omitted fields), status codes 201/204 accepted and 500 rejected, `slack set` with and without `--config` (Q2), `slack test` arg-count error.
- **Acceptance:** SL1–SL5 ticked.

### Phase 5: Discovery
- `discovery/mod.rs` (probe, parse, dedupe, host_port) and `discovery/info.rs` (SOAP + WS-Security + Basic fallback), plus `cli/discover.rs`.
- Tests: port `ParseProbeMatch`, `UniqueDevices`, `HostPort` (including `http://10.0.0.1:2020/onvif` → `10.0.0.1:2020` and bad URL → ""), `DiscoverTimeout`, `ParseProbeMatchError`. New: a WS-Security header test with a fixed nonce/time (digest known-answer), an info mock server returning 401 to WSSE then 200 to Basic (assert the fallback), the `DiscoverNoDevices` CLI test (`--timeout 10ms` → warning, exit 0), and a local-loopback responder test for end-to-end parsing (send the probe to a test socket by making the multicast address injectable).
- **Acceptance:** D1 and D2 ticked.

### Phase 6: Media foundation: RTSP session + test infrastructure
- Implement `media/session.rs`, `media/codec.rs`, `media/annexb.rs` (§6.12).
- **Test fixtures without ffmpeg** (`tests/support/fixtures.rs`): generate synthetic video at test time with the **OpenH264 encoder** (exposed by the same `openh264` crate). Use deterministic frames: a static gray background for N frames, then a moving white square, then a full-frame change, at 320×240, 10 fps, GOP 10. Encode H.265 test streams the same way if the chosen HEVC library has an encoder; otherwise commit a tiny (<50 KB) H.265 Annex-B fixture produced once offline and record its provenance in the ADR. Generate G.711 audio from a sine wave in code. For AAC passthrough/decode tests, commit a tiny (<10 KB) raw ADTS/AU fixture of a 1 kHz tone (or synthesize silent AAC-LC frames directly in Rust—an AAC silent SCE frame is a fixed ~7-byte bit pattern).
- **Fake RTSP server** (`tests/support/fake_rtsp.rs`, test-only, ~600 lines): OPTIONS/DESCRIBE/SETUP/PLAY/TEARDOWN; TCP interleaved and UDP; SDP with H.264 (+ sprop-parameter-sets), H.265, MJPEG, PCMA, mpeg4-generic AAC; RFC 6184 packetization (single NAL + FU-A), RFC 7798 (single + FU), RFC 2435, RFC 3640; Basic and Digest challenges (configurable: none/basic/digest/always-401); 404 for unknown paths; an optional TLS listener using an `rcgen` self-signed cert; scriptable faults (drop the connection after N packets, stall, refuse). It records every request so tests can assert the path/stream/auth used and that TEARDOWN was sent.
- Session tests: connect TCP/UDP, digest auth, wrong password → classified `auth`, refused port → `network-refused`, stall → `network-timeout`, unknown path → `not-found`, rtsps with a self-signed cert succeeds, credentials with special characters are decoded correctly, TEARDOWN observed.
- **Acceptance:** S7 and S9 ticked. The fake server is reusable by later phases.

### Phase 7: `snap`
- Implement `video_decode.rs` (trait + H.264/H.265/MJPEG), `yuv.rs`, `image_out.rs`, `snapshot.rs` and `cli/snap.rs`.
- Tests (all against the fake server, **binary run with `PATH=""`** to prove C2):
  - `SnapCreatesTempFile` (port): no `--out` → message, file exists, decodes as a 320×240 JPEG.
  - Standard mode picks a keyframe at ≥2 s (the fake server stamps frame indices into the picture content; assert which frame was captured by sampling pixels).
  - FirstKeyframe mode (`--rtsp-client gortsplib` and camera `rtsp_client: gortsplib`) captures the first keyframe.
  - Per-camera precedence (Q1): the camera says udp, no flag → the server observes a UDP SETUP. Flag tcp → TCP.
  - `--path` token reaches the server. `--stream stream2` reaches the server. Conflict error. `camera "x" not found`.
  - PNG/BMP/TIFF/WebP/PPM outputs decode; an unknown extension errors.
  - `--timeout 1s` against a stalling server → error containing `timeout`.
  - H.265 and MJPEG streams produce valid images.
  - Slack hook after a snap against the mock (file id printed).
  - Replace `TestSnapNoFFmpeg` with `snap_works_without_any_path`.
- **Acceptance:** S1–S6 and S10 ticked.

### Phase 8: `clip` and `audio`
- Implement `mux/{mp4,wav,adts}.rs`, `audio/*`, `recorder.rs`, `cli/record.rs`.
- Unit tests: G.711 tables (round-trip all 256 codes), G.726 against ITU test vectors (commit the small vector files), L16 swap, ADTS header bits, WAV header layout, MP4 box structure (parse back with the `mp4` dev-crate: track count, codec fourcc, timescale, sample count, duration ≈ D ± one frame, `stss` present).
- CLI tests (port and adapt): `ClipTempOutput`, `AudioCommandDefaults` (temp `.wav`, codec copy, PCMA → WAV fmt 6), `AudioCommandSeconds` (`--seconds 5` → duration ≈ 5 s), `AudioDoNotOverwriteExisting` (content unchanged, then `--overwrite` succeeds), `ClipAudioOnly` (`.wav`, 7 s), `AudioFlagsConflict` (both conflicts). New: `clip --overwrite=false` with an existing file → error; default clip overwrites; `--no-audio` → 1 track; default (`auto`) with AAC source into `.mp4` → 2 tracks (video + `mp4a` with valid ASC); default (`auto`) with PCMA source into `.mp4` → 1 track (video-only + stderr note, Q14); `--audio-codec copy` with PCMA into `.mp4` → error hint; `.mov` + copy/auto PCMA → 2 tracks; camera `no_audio: true` honored; camera `audio_codec` honored (including legacy `audio_codec: aac` fallback); Q6 implicit transcode note; AAC source + `.m4a` copy; H.265 into MP4 (`hvc1`); MJPEG into MOV; `--dur 3s --timeout 1s` succeeds (Q4); video starts on a keyframe (first sample in `stss`); a source without audio → video-only, no error.
- **Acceptance:** C1–C7 and AU1 ticked.

### Phase 9: `watch`
- Implement `scene.rs`, the watch pipeline and the action runner in `cli/watch.rs`.
- Tests: scene unit vectors plus a comparison with `scene_reference.json` (|Δ| ≤ 1e-6 per frame, assuming the same decoded frames; if decoder output differs slightly from ffmpeg's, allow |Δ| ≤ 0.005 and document why). Port `WatchMissingAction`. Threshold bounds. Against the fake server with the synthetic clip: `--duration 3s --json --action 'echo $CAMSNAP_SCORE >> <tmp>'` → JSON lines are valid, the score > threshold only at the scene change, the action file holds the matching scores, `CAMSNAP_CAMERA`/`CAMSNAP_TIME` are set, PATH is inherited (Q5), cooldown suppresses events, the template renders, the text format matches byte-for-byte except for the time, and a server drop → `stream ended: … (network-…)` with exit 1.
- **Acceptance:** W1–W8 ticked.

### Phase 10: `doctor`
- Implement `cli/doctor.rs` and `media/probe.rs`.
- Tests: port `DoctorNoCameras`. A refused port → `dial … failed`. A reachable fake server → `reachable at`. `--probe` with a wrong password → `probe failed: … (auth)` after 3 attempts (the server counts 3 DESCRIBEs). An invalid `--rtsp-auth` value is reported per camera.
- **Acceptance:** DR1 and DR2 ticked.

### Phase 11: Robustness, signals, polish
- Ctrl-C (`tokio::signal::ctrl_c`; on Unix also SIGTERM): snap aborts and removes the partial file; clip/audio **finalize** the container and keep it (C8), then exit 130; watch stops cleanly, kills the action children, sends TEARDOWN and exits 130.
- Confirm TEARDOWN happens in every exit path (fake server assertion).
- Memory bounds: bounded channels. Clip doesn't buffer the whole file in memory (streams to disk).
- Optional `RUST_LOG`-driven debug logging via `tracing` to stderr (off by default; no CLI change).
- Run `cargo deny` (licenses, advisories) and add `deny.toml` that allows the licenses chosen in Phase 0.
- **Acceptance:** C8 ticked. A manual soak: `watch` for 30 min against the fake server with flat RSS.

### Phase 12: Docs, packaging, Go removal, final audit
1. **Docs:** update `README.md` (install without ffmpeg, build from source with `cargo`, `--rtsp-client` legacy names, codec/container matrix, RTSPS cert note, the Q-decisions that change behavior), `SKILL.md` (remove "FFmpeg-based"; doctor no longer checks ffmpeg), `docs/spec.md` (architecture → Rust modules), `CHANGELOG.md` (0.4.0 entry listing the Q fixes), `docs/RELEASING.md` and `docs/releasing-homebrew.md` (cargo builds, **remove `depends_on "ffmpeg"`** from the formula), and replace `docs/idiomatic-go.md` with `docs/idiomatic-rust.md` (short house style: error handling, async rules, no panics in lib code).
2. **Build tooling:** rewrite the `Makefile` targets (`fmt`, `lint`, `test`, `build`, `build-all` for the C6 targets, `install` into `~/bin` with the same OS/arch detection) using cargo / cargo-zigbuild. Update the `package.json` scripts to their cargo equivalents. Replace `.goreleaser.yaml` + `release.yml` with a cargo-based release workflow (matrix build, `tar.gz` for Unix and `zip` for Windows, name template `camsnap_<version>_<os>_<arch>`, `checksums.txt`; identical artifact naming keeps the Homebrew tap scripts working).
3. **Remove Go:** delete `cmd/`, `internal/`, `go.mod`, `go.sum`, `.golangci.yml`, the Go CI workflow and `spikes/`.
4. **Final parity audit:** walk §3 row by row and check that each row has at least one automated test (link the test name in `docs/port-status.md`). Run the no-ffmpeg guard. Run the full test suite with `PATH=` emptied for the binary. Check binary linkage (`otool -L` / `ldd` / `dumpbin /dependents`: only OS libraries).
5. **Manual smoke checklist** (real hardware if available; document the results in the PR): a Tapo C225 (tcp, stream1), a Tapo TC70 (udp, stream2, `rtsp_client: gortsplib`), UniFi Protect with an `rtsps` token path, an H.265 camera. For each: `snap`, `clip --dur 5s`, `audio --seconds 5`, `watch --duration 60s`, `doctor --probe`, `discover --info`, and a Slack upload.

**Acceptance:** every §3 row ticked with a test reference, CI green on all OSes, the release dry-run produces all artifacts, and no Go files remain.

---

## 9. Enforcement: "no ffmpeg" guard (CI job)
Fail the build if any of these hold:
1. `cargo tree -e normal` contains any of `ffmpeg`, `rsmpeg`, `ac-ffmpeg`, `gstreamer`, `libav`.
2. `grep -RniE 'Command::new\("ffmpeg|ffprobe|avconv|gst-launch' src/` matches anything.
3. In `src/`, the string `ffmpeg` (case-insensitive) appears outside these allowed spots: the `ClientMode` alias table, comments that reference the algorithm source (e.g. `// port of ffmpeg vf_select get_scene_score`), and help texts that mention the legacy value name. Maintain this as an explicit allowlist file `ci/ffmpeg-allowlist.txt` (file:line patterns).
4. The integration test suite runs the binary with `PATH` set to an empty temp dir (Unix) and must pass.
5. The release-binary linkage check (Phase 12, item 4) finds only OS libraries.

---

## 10. Test plan summary (Go test → Rust test mapping)

| Go test | Rust test | Phase |
|---|---|---|
| `cmd/camsnap TestRootVersionFlag` | `tests/cli_root.rs::version_flag` | 3 |
| `TestAddAndList` | `cli_config.rs::add_and_list` | 3 |
| `TestSnapNoFFmpeg` | **replaced** by `cli_snap.rs::snap_works_without_any_path` | 7 |
| `TestSnapCreatesTempFile` | `cli_snap.rs::snap_creates_temp_file` | 7 |
| `TestRootVersionHelp` | `cli_root.rs::version_and_help` | 3 |
| `TestDoctorNoCameras` | `cli_doctor.rs::no_cameras` | 10 |
| `TestDiscoverNoDevices` | `cli_discover.rs::no_devices` | 5 |
| `TestWatchMissingAction` | `cli_watch.rs::missing_action` | 9 |
| `TestClipTempOutput` | `cli_record.rs::clip_temp_output` | 8 |
| `TestAudioCommandDefaults` | `cli_record.rs::audio_defaults` (asserts WAV + copy instead of ffmpeg args) | 8 |
| `TestAudioCommandSeconds` | `cli_record.rs::audio_seconds` (asserts ≈5 s duration) | 8 |
| `TestAudioDoNotOverwriteExisting` | `cli_record.rs::audio_no_overwrite` | 8 |
| `TestClipAudioOnly` | `cli_record.rs::clip_audio_only` | 8 |
| `TestAudioFlagsConflict` | `cli_record.rs::flag_conflicts` | 8 |
| `TestAppendStream` / `TestAppendPath` / `TestParseRTSPAuth` / `TestTransportFlag` | `cli::resolve` / `cli::flags` unit tests | 2 |
| `TestResolveSlackToken` / `TestResolveSlackChannel` | `cli::slack_cmd` unit tests | 4 |
| `TestParseSceneScore` | `cli::watch` unit test | 2 |
| config tests (6) | `config` unit tests | 2 |
| discovery tests (5) | `discovery` unit tests | 5 |
| `TestClassifyError`, `TestWithTimeoutZero` | `errors` / deadline unit tests | 2 |
| `TestHasBinary`, `TestAdaptFFmpegArgs` (4 subtests), `TestRunFFmpegFallbackToLegacy` | **obsolete** (ffmpeg shims, §3.4). Record this in `port-status.md` | — |
| rtsp url tests (4) | `rtsp_url` unit tests | 2 |
| `TestFindH264`, `TestFindH264None`, `TestGrabFrameViaGortBadURL`, `TestGrabFrameViaGortInvalidTransport` | `media::session::find_video_stream` tests + `snapshot` bad-URL / invalid-transport tests | 6–7 |
| `TestIsSlackID`, `TestResolveChannel` | `slack` unit + mock tests | 4 |

---

## 11. Definition of done
- [ ] Every row of §3 is ticked in `docs/port-status.md` with a linked automated test (or a documented manual check for hardware-only items).
- [ ] All §7 decisions are implemented as specified and mentioned in the CHANGELOG.
- [ ] The no-ffmpeg guard (§9) passes. The binary works with an empty `PATH`.
- [ ] `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` and `cargo deny check` are green on Linux, macOS and Windows.
- [ ] Release artifacts build for all C6 targets. Linux binaries are static (musl).
- [ ] Existing Go-era `config.yaml` files load and work unchanged.
- [ ] README, SKILL.md, spec, release docs and the Homebrew instructions no longer mention ffmpeg as a requirement.
- [ ] The Go sources and Go tooling are removed.
