# Meetbase architecture

```
meetbase/
├── crates/
│   └── transcribe-core/        # reusable engine (no Tauri dependency)
│       └── src/
│           ├── audio/
│           │   ├── capture.rs   # cpal mic capture; WASAPI loopback (Windows)
│           │   ├── mixer.rs     # lock-free ring-buffer mix of mic + system audio
│           │   ├── resample.rs  # anything → 16 kHz mono f32 (rubato)
│           │   ├── vad.rs       # VoiceActivityDetector trait + adaptive energy VAD
│           │   ├── chunker.rs   # VAD-driven utterance chunking for streaming STT
│           │   └── import.rs    # media file → samples (symphonia)
│           ├── transcribe/      # whisper.cpp wrapper (whisper-rs)
│           ├── models/          # model registry + downloader (sha256-verified)
│           └── llm/             # ChatProvider trait, Ollama + OpenAI-compat, templates
└── apps/desktop/
    ├── src/                     # React + TS + Tailwind UI
    │   ├── lib/                 # typed Tauri bindings (api.ts), zustand store
    │   └── views/               # Record, Library, Meeting, Settings
    └── src-tauri/
        ├── migrations/          # SQLite schema (sqlx)
        └── src/
            ├── commands.rs      # IPC surface (thin)
            ├── recording.rs     # live-session orchestration
            ├── worker.rs        # dedicated whisper thread
            ├── db.rs            # repositories
            ├── settings.rs      # typed settings (JSON in SQLite)
            └── export.rs        # Markdown rendering
```

## Data flow (live recording)

```
┌────────────── capture thread (owns !Send cpal streams) ──────────────┐
│ mic ──────────► MixerInput ─┐                                        │
│ system audio ─► MixerInput ─┴─► StreamMixer.drain() every 100 ms     │
│                                   └─► SpeechChunker (VAD)            │
└──────────────────────────────────────────┬───────────────────────────┘
                                  SpeechChunk (mpsc)
┌──────────── transcription task (async) ──▼───────────────────────────┐
│ TranscriberWorker.transcribe(chunk)   ← dedicated OS thread,         │
│   └─► segments → SQLite → emit "transcript-segment" → React UI       │
└───────────────────────────────────────────────────────────────────────┘
```

Design decisions worth knowing:

- **Everything downstream is 16 kHz mono f32** (Whisper's input format).
  Capture and import convert immediately; VAD/chunker/transcriber never
  worry about formats.
- **One whisper model instance, one thread.** whisper.cpp state is `Send`
  but not `Sync`, and models take seconds to load — `TranscriberWorker`
  owns the loaded model on a long-lived thread and serves jobs from a
  channel, reloading lazily when the configured model changes.
- **Chunking, not sliding windows.** The chunker cuts at silence
  boundaries (700 ms pause) with a 25 s force-cut cap, trims trailing
  silence, and pads with 200 ms of context. Chunks are transcribed with
  `no_context = true` so an error in one chunk can't poison the next.
- **The capture thread is the only place cpal types live.** Stream
  handles are `!Send`; the thread parks on a stop flag and drops them on
  exit. The mixer's ring buffers are the thread boundary.
- **LLM layer sends text only.** `transcribe-core::llm` knows nothing
  about audio. Providers implement one trait (`ChatProvider`); the two
  built-ins (Ollama, OpenAI-compatible) cover local and every common
  hosted endpoint.
- **Errors cross IPC as `{ message }`.** `AppError` serializes to a
  user-presentable message; the frontend re-throws as `Error`.

## Database

SQLite via sqlx (WAL mode), one file in the platform app-data dir.
Tables: `meetings`, `segments` (FK cascade), `summaries`, `settings`
(single JSON document keyed `app_settings`). Search is `LIKE` over
titles + segment text with escaped wildcards; FTS5 is a planned upgrade.

## Testing strategy

- **Pure-DSP units** (mixer, resampler, VAD, chunker) are tested on
  synthetic signals — sine bursts vs. silence — asserting durations,
  boundaries and adaptation behavior.
- **Model manager** is tested against a wiremock HTTP server, including
  checksum-mismatch cleanup and progress callbacks.
- **LLM adapters** are tested against wiremock (request shape, auth
  header, error surfacing).
- **Repositories & settings** run against throwaway SQLite files.
- **Whisper integration** has an `#[ignore]`d test that runs when a real
  model is downloaded (`cargo test -- --ignored`).

## The Pro / engine split

`transcribe-core` is MIT-licensed, so it can be reused freely outside
this repo. Pro features (diarization, more exporters, custom
templates, auto-start) are designed as additive modules on the same
interfaces — e.g. diarization will implement a post-processing pass over
`TranscriptSegment`s, and exporters consume the same `Meeting`/`Segment`
data the Markdown exporter uses.
