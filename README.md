<p align="center">
  <img src="apps/desktop/src-tauri/icons/128x128.png" alt="Meetbase" width="84" />
</p>

<h1 align="center">Meetbase</h1>

<p align="center">
  <strong>Privacy-first AI meeting notetaker. Your meetings never leave your machine.</strong>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-1ea795" alt="MIT" /></a>
  <img src="https://img.shields.io/badge/platform-macOS%20%C2%B7%20Windows-1ea795" alt="Platforms" />
  <img src="https://img.shields.io/badge/transcription-100%25%20local-1ea795" alt="Local" />
</p>

---

Meetbase records, transcribes and summarizes your meetings **entirely on your computer** — no bot joining your calls, no audio uploaded anywhere, no account required.

- 🎙 **One-click recording** with live transcription (Whisper, runs locally)
- 🗣 **Speaker recognition** — local diarization labels who said what (pyannote, ONNX)
- 📄 **Import** audio/video files (wav, mp3, m4a, mp4, mov, …) and get a transcript
- 🧠 **AI summaries** — notes, decisions, action items — via local **Ollama** or your own API key (OpenAI, Groq, OpenRouter, Anthropic-compatible). Only transcript *text* is ever sent, never audio
- 🌍 **Multilingual**: transcription in 90+ languages, summaries in the language you choose (including Polish)
- 🔎 **Searchable local history** — everything stored in a local SQLite file
- 📤 **Export to Markdown**
- 🔒 **Zero telemetry.** Open source under MIT

## Why not Otter / Fireflies / Granola?

Cloud notetakers send your most confidential conversations — legal advice, sales negotiations, health data — to third-party servers. That is a non-starter for lawyers, consultants, healthcare and anyone subject to GDPR. Meetbase keeps the entire pipeline (audio capture → speech-to-text → storage) on your machine. The only optional network call is the summary request to the LLM provider *you* configure — and a local Ollama model keeps even that offline.

## Getting started

### Download

Pre-built, signed installers for macOS and Windows are published on the [releases page](https://github.com/clovernetic/meetbase/releases).

### First run

1. Open **Settings → Speech recognition** and download a Whisper model.
   *Whisper Small* (~490 MB) is a good multilingual default; *Medium (quantized)* gives noticeably better Polish.
2. (Optional) Configure AI summaries: point Meetbase at a local [Ollama](https://ollama.com) server, or paste an API key for any OpenAI-compatible endpoint.
3. Hit **Start recording**.

### Build from source

Prerequisites: Rust ≥ 1.85, Node ≥ 22, pnpm ≥ 10, plus the [Tauri 2 platform setup](https://v2.tauri.app/start/prerequisites/).

```sh
git clone https://github.com/clovernetic/meetbase
cd meetbase
pnpm install
pnpm --filter @meetbase/desktop tauri dev          # development
pnpm --filter @meetbase/desktop tauri build        # release bundle
# GPU acceleration:
pnpm --filter @meetbase/desktop tauri build -- --features metal   # macOS
pnpm --filter @meetbase/desktop tauri build -- --features cuda    # Windows/NVIDIA
pnpm --filter @meetbase/desktop tauri build -- --features vulkan  # Windows/AMD+Intel
```

Run the test suite:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets
pnpm --filter @meetbase/desktop typecheck
```

## How it works

```
mic ─┐
     ├─ mix ─ resample 16 kHz ─ VAD ─ speech chunks ─ whisper.cpp ─ transcript ─ SQLite
sys ─┘                                                                   │
(loopback)                                            Ollama / BYOK LLM ─┴─ summary
```

The engine lives in [`crates/transcribe-core`](crates/transcribe-core) — a reusable Rust library (audio capture, VAD, chunking, Whisper, model management, LLM adapters) with the Tauri app in [`apps/desktop`](apps/desktop) as a thin shell on top. See [ARCHITECTURE.md](ARCHITECTURE.md) for the full picture.

### Platform notes

| Capability | macOS | Windows |
|---|---|---|
| Microphone capture | ✅ | ✅ |
| System-audio capture (other participants) | ✅ CoreAudio process tap (macOS 14.4+) | ✅ WASAPI loopback |
| GPU acceleration | ✅ Metal | ✅ CUDA / Vulkan |

On macOS the first recording asks for the system-audio permission; if you decline, macOS silently delivers empty audio for the system side (the microphone still works) — re-enable it under *System Settings → Privacy & Security → Screen & System Audio Recording*. On macOS < 14.4 Meetbase records the microphone only. Linux builds from source today; native packages are planned.

## Roadmap

- Auto-detect meetings & auto-start recording — Pro
- PDF / DOCX / Notion / Slack export — Pro
- Custom summary templates — Pro
- Cloud sync & "chat with your meetings" — optional subscription, opt-in

The free, open-source core is *actually useful*, not a crippled demo — and it stays that way.

## Recording etiquette ⚖️

Many jurisdictions require the consent of meeting participants before recording. Meetbase is a tool for taking notes of *your own* meetings — always tell people you're recording.

## Contributing

PRs and issues are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md). Every feature lands with tests.

## License

[MIT](LICENSE) © Damian Prochaska ([Clovernetic](https://clovernetic.com)).
