//! Static registry of supported Whisper GGML models.
//!
//! Checksums and sizes come from the Git-LFS pointers in
//! `huggingface.co/ggerganov/whisper.cpp` and must be updated together with
//! the URLs when bumping model versions.

#[derive(Debug, Clone)]
pub struct ModelInfo {
    /// Stable identifier used in settings and the CLI (e.g. `small`).
    pub id: &'static str,
    pub display_name: &'static str,
    pub file_name: &'static str,
    pub url: &'static str,
    /// Expected size, used for progress totals when the server omits
    /// `Content-Length`.
    pub size_bytes: u64,
    /// SHA-256 of the file; empty string disables verification (test only).
    pub sha256: &'static str,
    /// One-line guidance shown in the model picker.
    pub quality_hint: &'static str,
}

macro_rules! model {
    ($id:literal, $name:literal, $file:literal, $size:literal, $sha:literal, $hint:literal) => {
        ModelInfo {
            id: $id,
            display_name: $name,
            file_name: $file,
            url: concat!(
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/",
                $file
            ),
            size_bytes: $size,
            sha256: $sha,
            quality_hint: $hint,
        }
    };
}

/// ONNX models for speaker diarization (downloaded together on demand).
/// URLs point at the pyannote-rs release mirror; checksums computed from
/// those artifacts.
pub static DIARIZATION_REGISTRY: &[ModelInfo] = &[
    ModelInfo {
        id: "segmentation-3.0",
        display_name: "Pyannote Segmentation 3.0",
        file_name: "segmentation-3.0.onnx",
        url: "https://github.com/thewh1teagle/pyannote-rs/releases/download/v0.1.0/segmentation-3.0.onnx",
        size_bytes: 5_983_836,
        sha256: "b78fc48113bb46fd247ae6a9aea737079550c647638db961df7e0e1e9f4ba62e",
        quality_hint: "Detects speaker turns (who-spoke-when boundaries).",
    },
    ModelInfo {
        id: "wespeaker-embedding",
        display_name: "WeSpeaker CAM++ voice embeddings",
        file_name: "wespeaker_en_voxceleb_CAM++.onnx",
        url: "https://github.com/thewh1teagle/pyannote-rs/releases/download/v0.1.0/wespeaker_en_voxceleb_CAM%2B%2B.onnx",
        size_bytes: 29_292_684,
        sha256: "c46fad10b5f81e1aa4a60c162714208577093655076c5450f8c469e522ec54ef",
        quality_hint: "Tells speakers apart by voice fingerprint.",
    },
];

pub static MODEL_REGISTRY: &[ModelInfo] = &[
    model!(
        "tiny",
        "Whisper Tiny",
        "ggml-tiny.bin",
        77_691_713,
        "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
        "Fastest, English-leaning. Good for quick drafts on weak hardware."
    ),
    model!(
        "base",
        "Whisper Base",
        "ggml-base.bin",
        147_951_465,
        "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
        "Fast with acceptable accuracy for clear English speech."
    ),
    model!(
        "small",
        "Whisper Small",
        "ggml-small.bin",
        487_601_967,
        "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
        "Recommended default: solid multilingual accuracy on mid-range CPUs."
    ),
    model!(
        "small-q5_1",
        "Whisper Small (quantized)",
        "ggml-small-q5_1.bin",
        190_085_487,
        "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb",
        "Small accuracy at 40% of the size; ideal for laptops on battery."
    ),
    model!(
        "medium",
        "Whisper Medium",
        "ggml-medium.bin",
        1_533_763_059,
        "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208",
        "High accuracy incl. Polish; needs a strong CPU or GPU for live use."
    ),
    model!(
        "medium-q5_0",
        "Whisper Medium (quantized)",
        "ggml-medium-q5_0.bin",
        539_212_467,
        "19fea4b380c3a618ec4723c3eef2eb785ffba0d0538cf43f8f235e7b3b34220f",
        "Medium accuracy at a third of the size — best quality/speed balance."
    ),
    model!(
        "large-v3-turbo",
        "Whisper Large v3 Turbo",
        "ggml-large-v3-turbo.bin",
        1_624_555_275,
        "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69",
        "Best accuracy with turbo speedups; recommended with GPU acceleration."
    ),
    model!(
        "large-v3-turbo-q5_0",
        "Whisper Large v3 Turbo (quantized)",
        "ggml-large-v3-turbo-q5_0.bin",
        574_041_195,
        "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        "Near-best accuracy at a third of the size."
    ),
];
