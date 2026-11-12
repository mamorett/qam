# Port Status & Feature Traceability Matrix (`camsnap` v0.3.0 → `qam` v0.4.0)

This document tracks 100% user-facing feature parity between the legacy Go `camsnap` v0.3.0 codebase and the Rust `qam` v0.4.0 port, per §3 of `go-to-rust.md`.

## 3.1 Top-Level CLI & Global Flags

|Status|Go source|Feature|Rust target|Test|
|---|---|---|---|---|
|[x]|`cmd/camsnap/main.go`|Exit code `1` on any command error; print error to `stderr` cleanly|`src/main.rs`|`tests/cli_root.rs`|
|[x]|`internal/cli/root.go`|Binary name `qam`; short + long descriptions; command ordering; help template|`src/cli/mod.rs`|`tests/cli_root.rs` (`insta` snapshot)|
|[x]|`root.go` persistent flags|`--config` (`string`), `--rtsp-transport` (`tcp\|udp`, default `tcp`), `--rtsp-auth` (`auto\|basic\|digest`, default `auto`), `--timeout` (`duration`, default `10s`), `--debug` (`bool`), `--json` (`bool`), `--no-color` (`bool`)|`src/cli/flags.rs`|`tests/cli_root.rs`|
|[x]|`root.go:normalizeBoolFlagArgs`|Support `--flag=true`, `--flag=false`, `--flag=1`, `--flag=0` on boolean flags (`--overwrite`, `--audio`, `--with-audio`, `--debug`, `--json`, `--no-color`, `--info`, `--probe`, `--dry-run`, `--keep`)|`src/cli/flags.rs`|`tests/cli_root.rs::bool_equals_syntax`|
|[x]|`root.go:SilenceUsage/Errors`|Do not print usage on runtime errors; print single `Error: <msg>` line to `stderr` (matching Go `cmd.PrintErrln("Error:", err)`)|`src/main.rs`|`tests/cli_root.rs`|
|[x]|`root.go:argsIgnoreRemaining`|All subcommands except `slack test` ignore unexpected positional args|`src/cli/mod.rs`|`tests/cli_root.rs`|
|[x]|`internal/cli/color.go`|Color disabled if `--no-color`, `NO_COLOR != ""`, `TERM == "dumb"`, or stdout is not a TTY|`src/cli/style.rs`|`src/cli/style.rs` unit tests|
|[x]|`root.go`|Shell completions (`completion bash\|zsh\|fish\|powershell`) + `--version` / `-v` flag on root|`src/cli/completion.rs`|`tests/cli_root.rs`|

## 3.2 Configuration (`~/.config/qam/config.yaml` with fallback to `~/.config/camsnap/config.yaml`)

|Status|Go source|Feature|Rust target|Test|
|---|---|---|---|---|
|[x]|`internal/config/config.go:DefaultPath`|`$XDG_CONFIG_HOME/qam/config.yaml` else `~/.config/qam/config.yaml` (fallback to `camsnap/config.yaml` on load if `qam/config.yaml` is absent)|`src/config.rs`|`src/config.rs::tests::default_path_*`|
|[x]|`config.go:Load`|Missing file returns empty `Config{}` (not error); trims fields; sets default `port=554`, `scheme=rtsp`|`src/config.rs`|`src/config.rs::tests::load_missing`|
|[x]|`config.go:Save`|Creates dir `0700`, writes file with `0600` perms (Unix), normalizes defaults before marshaling|`src/config.rs`|`src/config.rs::tests::save_permissions`|
|[x]|`config.go:UpsertCamera`|Case-insensitive name match; updates in-place or appends|`src/config.rs`|`src/config.rs::tests::upsert_*`|
|[x]|`config.go:FindCamera`|Case-insensitive lookup by name|`src/config.rs`|`src/config.rs::tests::find_*`|
|[x]|`Camera` YAML schema|Fields: `name`, `host`, `port,omitempty`, `user,omitempty`, `pass,omitempty`, `path,omitempty`, `scheme,omitempty`, `url,omitempty`, `rtsp_transport,omitempty`, `rtsp_auth,omitempty`, `threshold,omitempty`, `clip_duration,omitempty`, `cooldown,omitempty` (`duration` serialized as Go duration string e.g. `5s` or `0s`)|`src/config.rs`|`src/config.rs::tests::yaml_roundtrip_go_compat`|
|[x]|`Defaults` YAML schema|`rtsp_transport`, `rtsp_auth`, `timeout`, `threshold`, `clip_duration`, `cooldown`|`src/config.rs`|`src/config.rs::tests::yaml_roundtrip_go_compat`|

## 3.3 RTSP URL Building & Redaction

|Status|Go source|Feature|Rust target|Test|
|---|---|---|---|---|
|[x]|`internal/rtsp/url.go:BuildURL`|If `cam.URL != ""`, use it; if it lacks userinfo and `cam.User != ""`, inject URL-escaped `user[:pass]@`; else build `<scheme>://[user[:pass]@]host[:port][/path]`; reject empty host and path without host|`src/rtsp_url.rs`|`src/rtsp_url.rs::tests::build_url_*`|
|[x]|`url.go:RedactURL`|Replace password with `***`; fallback string replacement when `url::Url::parse` fails|`src/rtsp_url.rs`|`src/rtsp_url.rs::tests::redact_url_*`|

## 3.4 Precedence & Option Resolution

|Status|Go source|Feature|Rust target|Test|
|---|---|---|---|---|
|[x]|`helpers.go:resolveCamera`|`--camera <name>` \| `--url <url>` \| `--host <host>` (mutually exclusive; at least one required); `--user/--pass/--port/--path/--scheme` override stored camera fields|`src/cli/resolve.rs`|`src/cli/resolve.rs::tests::*`|
|[x]|`helpers.go:resolveRTSPTransport`|Flag (if explicitly changed) > `cam.RTSPTransport` > `cfg.Defaults.RTSPTransport` > flag default (`tcp`)|`src/cli/resolve.rs`|`src/cli/resolve.rs::tests::transport_precedence`|
|[x]|`helpers.go:resolveRTSPAuth`|Flag (if explicitly changed) > `cam.RTSPAuth` > `cfg.Defaults.RTSPAuth` > flag default (`auto`); normalize & validate `auto\|basic\|digest`|`src/cli/resolve.rs`|`src/cli/resolve.rs::tests::auth_precedence`|
|[x]|`helpers.go:resolveTimeout`|Flag (if explicitly changed) > `cfg.Defaults.Timeout` (if `>0`) > flag default (`10s`)|`src/cli/resolve.rs`|`src/cli/resolve.rs::tests::timeout_precedence`|
|[ ]|`watch.go` resolution|`threshold`, `clip_duration`, `cooldown`: Flag (if changed) > `cam.*` > `cfg.Defaults.*` > flag default (`0.25`, `0s`, `10s`); reject `threshold <= 0 \|\| threshold >= 1`|`src/cli/watch.rs`|`src/cli/watch.rs::tests::*`|

## 3.5 Subcommands

### `add` (`internal/cli/add.go`)

|Status|Go source|Feature|Rust target|Test|
|---|---|---|---|---|
|[x]|`add.go`|Flags: `--name` (required), `--host`, `--url`, `--port` (`554`), `--user`, `--pass`, `--path`, `--scheme` (`rtsp`), `--rtsp-transport`, `--rtsp-auth`, `--threshold` (`0`), `--clip-duration` (`0`), `--cooldown` (`0`)|`src/cli/add.rs`|`tests/cli_config.rs`|
|[x]|`add.go`|Validate `--name` non-empty, `--host` or `--url` required, validate `--rtsp-transport` & `--rtsp-auth` if set; save config; print `Saved camera <name> -> <config_path>`|`src/cli/add.rs`|`tests/cli_config.rs`|

### `list` / `ls` (`internal/cli/list.go`)

|Status|Go source|Feature|Rust target|Test|
|---|---|---|---|---|
|[x]|`list.go`|Aliases: `ls`. If empty: print `No cameras configured.` (or `[]` when `--json`). Otherwise print `<name>: <redacted_url>` per camera (or JSON array of `Camera` structs with password redacted as `***` in `pass` and `url`)|`src/cli/list.rs`|`tests/cli_config.rs`|

### `version` (`internal/cli/version.go`)

|Status|Go source|Feature|Rust target|Test|
|---|---|---|---|---|
|[x]|`version.go`|Prints version string (`0.4.0`, overrideable at build time via `QAM_VERSION` or `CAMSNAP_VERSION` env in `build.rs`)|`src/version.rs`|`tests/cli_root.rs`|

### `snap` (`internal/cli/snap.go`)

|Status|Go source|Feature|Rust target|Test|
|---|---|---|---|---|
|[ ]|`snap.go`|Flags: `--camera/--url/--host/--port/--user/--pass/--path/--scheme`, `--out` (`-` for stdout, empty -> temp file `qam-*.jpg`), `--overwrite` (default `true`), `--quality` (`0` = default quality ~85; `1..=31` mapped to JPEG quality: q=1 -> 98, q=31 -> 20)|`src/cli/snap.rs`|`tests/cli_snap.rs`|
|[ ]|`snap.go` + `ffmpeg.go:Snapshot`|Connect via RTSP, decode first complete video keyframe (H.264, H.265, or MJPEG), encode to JPEG (or PNG if `--out *.png`, WebP if `*.webp`), write atomically; refuse overwrite if `--overwrite=false` and file exists (`output file exists: <path>`); print output path (nothing when `--out -` unless `--json`); `--json` prints `{"path": "..."}`|`src/media/snapshot.rs`|`tests/cli_snap.rs`|
|[ ]|`snap.go` + `slack.go`|Slack upload flags (`--slack-channel`, `--slack-Message`, `--slack-thread-ts`, `--slack-token`, `--slack-api-url`, `--slack-dry-run`, `--slack-keep`); rejects `--out -` with `--slack-channel`; deletes temp file after upload unless `--out` was explicit or `--slack-keep`; `--json` outputs `{"path":..., "slack":{...}}`|`src/cli/snap.rs`, `src/slack.rs`|`tests/cli_slack.rs`|

### `clip` (`internal/cli/clip.go` + `record.go`)

|Status|Go source|Feature|Rust target|Test|
|---|---|---|---|---|
|[ ]|`clip.go` + `record.go`|Flags: camera flags + `--out` (default temp `qam-*.mp4`; `-` rejected with `stdout output not supported for clip`), `--duration` (default `5s`, must be `>0`), `--audio` (default `false`), `--with-audio` (alias for `--audio`, marks deprecated if both set), `--overwrite` (default `true`), Slack flags|`src/cli/record.rs`|`tests/cli_record.rs`|
|[ ]|`ffmpeg.go:RecordClip`|Record video (H.264 / H.265 stream-copied into MP4 without re-encoding; MJPEG decoded+encoded or rejected with clear message) for `--duration` starting from first keyframe; if `--audio`: include AAC/Opus directly, or decode G.711/G.726/L16 and transcode or store in MP4; write `+faststart` (`moov` before `mdat` or finalized cleanly); Ctrl-C finalizes trailer and exits `130`|`src/media/recorder.rs`, `src/media/mux/mp4.rs`|`tests/cli_record.rs`|

### `audio` (`internal/cli/audio.go` + `record.go`)

|Status|Go source|Feature|Rust target|Test|
|---|---|---|---|---|
|[ ]|`audio.go` + `record.go`|Flags: camera flags + `--out` (default temp `qam-*.wav`; `-` rejected with `stdout output not supported for audio`), `--duration` (default `5s`, must be `>0`), `--overwrite` (default `true`), Slack flags|`src/cli/record.rs`|`tests/cli_record.rs`|
|[ ]|`ffmpeg.go:RecordAudio`|Output format inferred from `--out` extension: `.wav` -> PCM s16le WAV (`hound`/custom writer); `.aac` / `.m4a` -> ADTS or M4A (copy when source is AAC); `.opus` / `.ogg` -> Ogg Opus (when source is Opus); `.mp4` -> audio-only MP4. If camera has no audio track, return clear error `no audio stream found in RTSP presentation`|`src/media/recorder.rs`, `src/media/mux/wav.rs`|`tests/cli_record.rs`|

### `watch` (`internal/cli/watch.go`)

|Status|Go source|Feature|Rust target|Test|
|---|---|---|---|---|
|[ ]|`watch.go`|Flags: camera flags + `--threshold` (`0.25`), `--clip-duration` (`0s`), `--clip-out` (dir), `--cooldown` (`10s`), `--action` (shell cmd via `sh -c`), `--duration` (`0s` = until Ctrl-C), `--audio` (`false`), Slack flags (requires `--clip-duration > 0` if `--slack-channel` set)|`src/cli/watch.rs`|`tests/cli_watch.rs`|
|[ ]|`ffmpeg.go:WatchScene` + `watch.go`|Decode RTSP video frames (throttled to ~2 fps / every 500ms or keyframe interval), compute scene-change score in `[0.0, 1.0]` matching `ffmpeg` `select='gte(scene,T)'` (`mafd` / `diff` formula); when `score >= threshold` and cooldown elapsed: print `Motion score=0.32` (or JSON event), run `--action` with `QAM_CAMERA`, `QAM_SCORE`, `QAM_TIME` (and legacy `CAMSNAP_CAMERA`, `CAMSNAP_SCORE`, `CAMSNAP_TIME`) + args `<camera> <score> <rfc3339>`, and if `--clip-duration > 0` record clip `<camera>-<YYYYMMDD-HHMMSS>.mp4` (in `--clip-out` or temp dir) + optional Slack upload|`src/media/scene.rs`, `src/cli/watch.rs`|`tests/cli_watch.rs`|

### `discover` (`internal/cli/discover.go` + `internal/discovery/*`)

|Status|Go source|Feature|Rust target|Test|
|---|---|---|---|---|
|[x]|`discover.go` + `discovery.go`|WS-Discovery probe to `239.255.255.250:3702`, parse `XAddrs`, deduplicate, print host and suggested `add` command; `--timeout` (default 3 s); `--info` attempts GetDeviceInformation with WS-Security digest and Basic fallback using config credentials|`src/discovery/mod.rs`, `src/discovery/info.rs`, `src/cli/discover.rs`|`tests/cli_discover.rs`|

### `doctor` (`internal/cli/doctor.go` + `internal/rtspclient/client.go`)

|Status|Go source|Feature|Rust target|Test|
|---|---|---|---|---|
|[ ]|`doctor.go` + `rtspclient/client.go`|Flags: `--camera/--url/--host/...`, `--probe` (default `true`). Does **not** check for `ffmpeg` on `PATH` (instead reports built-in media engine status / codecs). If no camera configured and no target flags, prints `No cameras configured.` (or `{"cameras":[]}`). For each target camera: DNS lookup, RTSP `DESCRIBE` (plus short stream probe when `--probe`: setup + play + receive 1st packet/frame to verify transport/codec), report stream codecs (`H264`, `H265`, `AAC`, `PCMA`, etc.), classify errors (`401`/`403` auth hint, `404` path hint, timeout transport hint, UDP drop hint); `--json` outputs `DoctorReport` JSON|`src/cli/doctor.rs`, `src/media/probe.rs`|`tests/cli_doctor.rs`|

### `slack` (`internal/cli/slackcmd.go` + `internal/slack/slack.go`)

|Status|Go source|Feature|Rust target|Test|
|---|---|---|---|---|
|[x]|`slackcmd.go` + `slack.go`|`qam slack set --token --channel`, `qam slack test <file> [--channel] [--message]`. Channel resolution with pagination. Token & channel precedence. 3-step file upload flow (`files.getUploadURLExternal` -> POST raw bytes -> `files.completeUploadExternal`)|`src/slack.rs`, `src/cli/slack_cmd.rs`|`tests/cli_slack.rs`|
