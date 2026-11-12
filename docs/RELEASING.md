# Releasing `qam`

## Release Process

1. **Verify Quality Gates**:
   ```sh
   cargo fmt --check
   cargo clippy --all-targets -- -D warnings
   cargo test
   ./ci/check-no-ffmpeg.sh
   ```

2. **Update Version**:
   - Update `version` in `Cargo.toml`.
   - Update `CHANGELOG.md` with release notes.

3. **Tag Release**:
   ```sh
   git tag -a v0.4.0 -m "Release v0.4.0"
   git push origin v0.4.0
   ```

4. **CI Release Workflow**:
   The GitHub Actions workflow builds cross-platform static binaries for:
   - `aarch64-apple-darwin`
   - `x86_64-apple-darwin`
   - `x86_64-unknown-linux-gnu`
   - `aarch64-unknown-linux-gnu`
   - `x86_64-pc-windows-msvc`
