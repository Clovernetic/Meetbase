//! End-to-end pipeline test: synthesized speech → decode → whisper → text.
//!
//! Requires the `tiny` model to be downloaded and (on macOS) the `say` TTS
//! command, so it is `#[ignore]`d in normal runs:
//!
//! ```sh
//! cargo test -p transcribe-core --test pipeline -- --ignored
//! ```

use std::process::Command;
use std::time::Duration;

use transcribe_core::audio::import::decode_to_pipeline_format;
use transcribe_core::models::ModelManager;
use transcribe_core::transcribe::{TranscribeOptions, Transcriber};

#[test]
#[ignore = "requires the `tiny` model and macOS `say`"]
fn synthesized_speech_is_transcribed() {
    let manager = ModelManager::with_default_dir().expect("model dir");
    let model_path = match manager.resolve("tiny") {
        Ok(p) => p,
        Err(_) => {
            eprintln!("skipping: tiny model not downloaded");
            return;
        }
    };

    // Synthesize a known sentence to an AIFF file.
    let dir = tempfile::tempdir().unwrap();
    let aiff = dir.path().join("fixture.aiff");
    let sentence = "The quick brown fox jumps over the lazy dog.";
    let status = Command::new("say")
        .args(["-o"])
        .arg(&aiff)
        .arg(sentence)
        .status();
    let Ok(status) = status else {
        eprintln!("skipping: `say` unavailable");
        return;
    };
    assert!(status.success(), "say failed");

    // Decode → 16 kHz mono.
    let samples = decode_to_pipeline_format(&aiff).expect("decode aiff");
    assert!(
        samples.len() > 16_000,
        "fixture suspiciously short: {} samples",
        samples.len()
    );

    // Transcribe.
    let mut transcriber = Transcriber::load(&model_path, "tiny").expect("load model");
    let segments = transcriber
        .transcribe(
            &samples,
            Duration::ZERO,
            &TranscribeOptions {
                language: Some("en".into()),
                ..TranscribeOptions::default()
            },
        )
        .expect("transcribe");

    let text = segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    println!("transcript: {text}");

    // The tiny model garbles edges sometimes; require the distinctive words.
    for word in ["quick", "brown", "fox", "lazy", "dog"] {
        assert!(text.contains(word), "missing `{word}` in: {text}");
    }
    // Timestamps must be sane and ordered.
    for pair in segments.windows(2) {
        assert!(pair[0].start_ms <= pair[1].start_ms);
    }
}
