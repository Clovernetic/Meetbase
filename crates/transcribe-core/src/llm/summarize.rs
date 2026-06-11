//! Meeting summarization: prompt construction and template definitions.
//!
//! Templates are deliberately data, not code — the Pro tier adds custom
//! user-defined templates on top of the same structure.

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::transcribe::TranscriptSegment;

use super::{ChatMessage, ChatProvider};

/// A summary template: what the model is asked to extract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryTemplate {
    pub id: String,
    pub name: String,
    /// Section instructions injected into the system prompt.
    pub instructions: String,
}

/// Built-in templates available in the free tier.
pub fn builtin_templates() -> Vec<SummaryTemplate> {
    vec![
        SummaryTemplate {
            id: "standard".into(),
            name: "Meeting notes".into(),
            instructions: "\
Produce these sections:
## Summary
A 2-4 sentence overview of what the meeting was about and what was achieved.
## Key points
Bullet list of the important topics, facts and arguments discussed.
## Decisions
Bullet list of decisions that were made. If none, write \"No decisions recorded.\"
## Action items
Bullet list in the form \"- [ ] owner — task (deadline if mentioned)\". \
If the owner is unclear, omit the owner rather than guessing."
                .into(),
        },
        SummaryTemplate {
            id: "brief".into(),
            name: "Brief TL;DR".into(),
            instructions: "\
Produce a single short paragraph (max 5 sentences) capturing the essence of \
the meeting, followed by an '## Action items' checklist if any tasks were \
agreed."
                .into(),
        },
        SummaryTemplate {
            id: "interview".into(),
            name: "Interview / research call".into(),
            instructions: "\
Produce these sections:
## Context
Who was interviewed and why (1-2 sentences).
## Insights
Bullet list of the most important things learned, each with a short verbatim \
quote where one exists.
## Follow-ups
Bullet list of open questions and agreed next steps."
                .into(),
        },
    ]
}

/// Formats a transcript for the prompt as `[mm:ss] text` lines.
pub fn format_transcript(segments: &[TranscriptSegment]) -> String {
    segments
        .iter()
        .map(|s| {
            let secs = s.start_ms / 1000;
            format!("[{:02}:{:02}] {}", secs / 60, secs % 60, s.text)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Builds the chat messages for a summarization request.
///
/// `language` is the *output* language ("English", "Polish", …) — the
/// transcript may be in any language.
pub fn build_messages(
    segments: &[TranscriptSegment],
    template: &SummaryTemplate,
    language: &str,
) -> Vec<ChatMessage> {
    let system = format!(
        "You are an expert meeting analyst. You will receive a raw meeting \
transcript with [mm:ss] timestamps. It may contain transcription errors — \
read through them. Do not invent facts that are not in the transcript.\n\n\
{}\n\nWrite the entire output in {language}. Output clean Markdown only, \
no preamble.",
        template.instructions
    );
    vec![
        ChatMessage::system(system),
        ChatMessage::user(format!("Transcript:\n\n{}", format_transcript(segments))),
    ]
}

/// Runs summarization end to end against the given provider.
pub async fn summarize(
    provider: &dyn ChatProvider,
    segments: &[TranscriptSegment],
    template: &SummaryTemplate,
    language: &str,
) -> Result<String> {
    let messages = build_messages(segments, template, language);
    provider.complete(&messages).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start_ms: u64, text: &str) -> TranscriptSegment {
        TranscriptSegment {
            text: text.into(),
            start_ms,
            end_ms: start_ms + 1000,
        }
    }

    #[test]
    fn transcript_formatting_uses_mm_ss() {
        let formatted = format_transcript(&[
            seg(0, "Hello everyone."),
            seg(65_000, "Let's get started."),
            seg(3_600_000, "One hour in."),
        ]);
        assert_eq!(
            formatted,
            "[00:00] Hello everyone.\n[01:05] Let's get started.\n[60:00] One hour in."
        );
    }

    #[test]
    fn messages_carry_template_and_language() {
        let templates = builtin_templates();
        let messages = build_messages(&[seg(0, "Hi")], &templates[0], "Polish");
        assert_eq!(messages.len(), 2);
        assert!(messages[0].content.contains("## Action items"));
        assert!(messages[0].content.contains("in Polish"));
        assert!(messages[1].content.contains("[00:00] Hi"));
    }

    #[test]
    fn builtin_templates_have_unique_ids() {
        let templates = builtin_templates();
        let mut ids: Vec<&str> = templates.iter().map(|t| t.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), templates.len());
    }
}
