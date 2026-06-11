# Contributing to Meetbase

Thanks for helping! A few ground rules keep the project healthy.

## Setup

Follow the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS, then:

```sh
pnpm install
pnpm --filter @meetbase/desktop tauri dev
```

## Before you open a PR

1. **Every feature or fix ships with tests.** No exceptions — see
   [ARCHITECTURE.md](ARCHITECTURE.md#testing-strategy) for what kind of
   test fits where.
2. The full check suite must pass:

   ```sh
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   pnpm --filter @meetbase/desktop typecheck
   pnpm --filter @meetbase/desktop build
   ```

3. Keep PRs focused — one logical change per PR.
4. Commit messages: conventional-commits style (`feat:`, `fix:`, `docs:`, …).

## Where things live

- Engine logic (audio, transcription, models, LLM) → `crates/transcribe-core`
- Anything Tauri/SQLite/UI-specific → `apps/desktop`
- If a change could be useful outside the desktop app, it belongs in the engine.

## Privacy invariants (non-negotiable)

- Audio never leaves the machine. Ever.
- Network calls are allowed only for: model downloads (HuggingFace) and
  user-configured LLM providers (text only).
- No telemetry. If you want usage stats, propose an explicit opt-in design first.

## Reporting bugs

Include OS + version, what you did, what happened, and relevant logs
(`RUST_LOG=debug` prints engine traces to the console in dev mode).

## License

By contributing you agree your contributions are licensed under MIT.
