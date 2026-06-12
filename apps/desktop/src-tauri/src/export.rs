//! Markdown export of meetings (free tier). PDF/DOCX/Notion/Slack land in
//! the Pro layer on top of the same data.

use crate::db::{Meeting, Segment, Summary};

fn format_ts(ms: i64) -> String {
    let total_secs = ms / 1000;
    let (h, m, s) = (total_secs / 3600, (total_secs % 3600) / 60, total_secs % 60);
    if h > 0 {
        format!("{h:02}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

fn format_duration(ms: i64) -> String {
    let mins = ms / 60_000;
    if mins >= 60 {
        format!("{} h {} min", mins / 60, mins % 60)
    } else {
        format!("{mins} min")
    }
}

/// Renders a complete meeting export: metadata, latest summary, transcript.
pub fn meeting_to_markdown(
    meeting: &Meeting,
    segments: &[Segment],
    summary: Option<&Summary>,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", meeting.title));
    out.push_str(&format!(
        "*{}* · {} · transcribed locally with [Meetbase](https://github.com/clovernetic/meetbase)\n\n",
        &meeting.created_at[..meeting.created_at.len().min(10)],
        format_duration(meeting.duration_ms),
    ));

    if let Some(summary) = summary {
        out.push_str(summary.content.trim());
        out.push_str("\n\n");
    }

    out.push_str("## Transcript\n\n");
    if segments.is_empty() {
        out.push_str("*No speech was detected in this meeting.*\n");
    } else {
        for seg in segments {
            match seg.speaker {
                Some(speaker) => out.push_str(&format!(
                    "**[{}] Speaker {speaker}:** {}\n\n",
                    format_ts(seg.start_ms),
                    seg.text
                )),
                None => out.push_str(&format!(
                    "**[{}]** {}\n\n",
                    format_ts(seg.start_ms),
                    seg.text
                )),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meeting() -> Meeting {
        Meeting {
            id: "m1".into(),
            title: "Quarterly planning".into(),
            created_at: "2026-06-11T10:00:00Z".into(),
            duration_ms: 65 * 60_000,
            source: "live".into(),
            language: Some("en".into()),
        }
    }

    fn seg(start_ms: i64, text: &str) -> Segment {
        Segment {
            id: 0,
            meeting_id: "m1".into(),
            start_ms,
            end_ms: start_ms + 1000,
            text: text.into(),
            speaker: None,
        }
    }

    #[test]
    fn renders_header_summary_and_transcript() {
        let summary = Summary {
            id: 1,
            meeting_id: "m1".into(),
            template_id: "standard".into(),
            language: "English".into(),
            content: "## Summary\nWe planned Q3.".into(),
            provider: "ollama".into(),
            model: "llama3.2".into(),
            created_at: "2026-06-11T11:00:00Z".into(),
        };
        let md = meeting_to_markdown(
            &meeting(),
            &[seg(0, "Welcome."), seg(3_725_000, "Wrapping up.")],
            Some(&summary),
        );
        assert!(md.starts_with("# Quarterly planning\n"));
        assert!(md.contains("2026-06-11"));
        assert!(md.contains("1 h 5 min"));
        assert!(md.contains("## Summary\nWe planned Q3."));
        assert!(md.contains("**[00:00]** Welcome."));
        // Timestamps past the hour use h:mm:ss.
        assert!(md.contains("**[01:02:05]** Wrapping up."));
    }

    #[test]
    fn renders_speaker_labels_when_present() {
        let mut labeled = seg(0, "Hi there.");
        labeled.speaker = Some(1);
        let md = meeting_to_markdown(&meeting(), &[labeled, seg(2000, "Anonymous line.")], None);
        assert!(md.contains("**[00:00] Speaker 1:** Hi there."));
        assert!(md.contains("**[00:02]** Anonymous line."));
    }

    #[test]
    fn renders_without_summary_and_empty_transcript() {
        let md = meeting_to_markdown(&meeting(), &[], None);
        assert!(md.contains("No speech was detected"));
        assert!(!md.contains("## Summary"));
    }
}
