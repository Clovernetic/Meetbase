# Meetbase — agent notes

Privacy-first local AI meeting notetaker. Tauri 2 + React desktop app on top
of the `transcribe-core` Rust engine. Read ARCHITECTURE.md first.

## Commands

```sh
cargo test --workspace                                  # all Rust tests
cargo clippy --workspace --all-targets -- -D warnings   # must be clean
cargo fmt --all
pnpm --filter @meetbase/desktop typecheck               # TS
pnpm --filter @meetbase/desktop build                   # frontend build
pnpm --filter @meetbase/desktop tauri dev               # run the app
```

## Hard rules

- Every feature/fix ships with tests (PRD requirement; CI enforces).
- Privacy invariants: audio never leaves the machine; network only for
  model downloads + user-configured LLM (text only); no telemetry.
- Engine logic goes in `crates/transcribe-core` (no Tauri deps there);
  app-specific code in `apps/desktop`.
- The whole audio pipeline runs at 16 kHz mono f32 after capture/import.
- cpal stream handles are `!Send` — they must stay on the capture thread
  (see `recording.rs`).
- whisper state lives on the single `TranscriberWorker` thread; don't
  load models elsewhere.

## Gotchas

- `serde` casing: commands/events use camelCase payloads; `ProviderConfig`
  is tagged `kind` ("ollama" / "open_ai_compat") with camelCase fields.
  TS mirrors live in `apps/desktop/src/lib/types.ts` — keep in sync.
- Model registry checksums come from HuggingFace LFS pointers
  (`https://huggingface.co/ggerganov/whisper.cpp/raw/main/<file>`).
- GPU features are opt-in cargo features (`metal`, `cuda`, `vulkan`)
  threaded through `meetbase → transcribe-core → whisper-rs`.
- System-audio capture: macOS uses a CoreAudio process tap via `cidre`
  (`audio/system_macos.rs`, needs macOS 14.4+; permission denial = silent
  zeros, not an error); Windows uses WASAPI loopback via cpal.
