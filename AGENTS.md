# Project Guidelines

## Code Style
- Follow idiomatic Rust and the existing synchronous design; prefer the current Arc/Mutex/RwLock patterns over introducing an async runtime.
- Keep user-facing labels, errors, and logs translatable through rust_i18n::t! instead of hardcoding strings in Rust files.
- Preserve the current module split in src/: config, event, log, process, tray, and watcher each own a separate concern.
- Respect rustfmt.toml. CI expects nightly rustfmt because the project uses unstable formatting options.

## Architecture
- This project is a desktop tray application, not a long-running service with an HTTP API. The main entrypoint in src/main.rs installs color-eyre, initializes file logging, sets the locale, loads config, and starts the tray app.
- src/tray/app.rs owns the winit event loop, tray icon lifecycle, menu refresh flow, and config reload orchestration.
- src/process.rs is the only place that should start, stop, and track managed child processes. Preserve the Windows Job Object cleanup behavior when changing process lifecycle code.
- src/config.rs loads a portable config.json next to the executable first; otherwise it falls back to ~/.config/servicetray.json.
- src/watcher.rs reloads configuration and emits custom events. Tray menu updates are driven from config changes rather than manual state duplication.
- locales/app.yml is the source of truth for user-visible strings.

## Build and Test
- Use cargo check --all-targets --all-features for routine validation.
- Use cargo test --all-features when behavior changes.
- Use cargo clippy --all-targets --all-features -- -D warnings before finishing non-trivial Rust changes.
- Format with cargo +nightly fmt --all. The CI workflow uses nightly rustfmt for the same reason.
- Use cargo run for local execution. Set RUST_LOG=info when you need more tracing output; .cargo/config.toml already enables backtraces.
- On Linux, tray-related dependencies require libgtk-3-dev libxdo-dev libappindicator3-dev, matching the CI workflow.

## Conventions
- Keep the logging guard returned by log::init() alive for the whole program lifetime.
- When adding config fields, update serde models in src/config.rs and the reload path in src/watcher.rs and src/tray/app.rs.
- When changing tray behavior, expect to touch both src/tray/app.rs and src/tray/menu.rs.
- When adding or changing any user-visible text, update locales/app.yml in the same change.
- Do not edit runtime or build output under logs/ or target/ unless the task is explicitly about generated artifacts.