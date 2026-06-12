//! SQLite persistence layer.
//!
//! All meeting data lives in one SQLite file under the app data directory.
//! Queries are plain `sqlx` (runtime-checked) so the crate builds without a
//! database present; the schema is owned by the migrations in `migrations/`.

use std::path::Path;

use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};

use crate::error::{AppError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meeting {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub duration_ms: i64,
    pub source: String,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub id: i64,
    pub meeting_id: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub speaker: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: i64,
    pub meeting_id: String,
    pub template_id: String,
    pub language: String,
    pub content: String,
    pub provider: String,
    pub model: String,
    pub created_at: String,
}

/// A meeting list entry with a search-context snippet.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingListItem {
    #[serde(flatten)]
    pub meeting: Meeting,
    pub segment_count: i64,
    pub has_summary: bool,
    /// First matching transcript line when a search query was given.
    pub snippet: Option<String>,
}

pub async fn open(db_path: &Path) -> Result<SqlitePool> {
    if let Some(parent) = db_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .map_err(AppError::from)?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

// ---- meetings ----

pub async fn create_meeting(
    pool: &SqlitePool,
    id: &str,
    title: &str,
    source: &str,
    language: Option<&str>,
) -> Result<Meeting> {
    sqlx::query("INSERT INTO meetings (id, title, source, language) VALUES (?, ?, ?, ?)")
        .bind(id)
        .bind(title)
        .bind(source)
        .bind(language)
        .execute(pool)
        .await?;
    get_meeting(pool, id).await
}

pub async fn get_meeting(pool: &SqlitePool, id: &str) -> Result<Meeting> {
    let row = sqlx::query(
        "SELECT id, title, created_at, duration_ms, source, language FROM meetings WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("meeting {id}")))?;
    Ok(meeting_from_row(&row))
}

fn meeting_from_row(row: &sqlx::sqlite::SqliteRow) -> Meeting {
    Meeting {
        id: row.get("id"),
        title: row.get("title"),
        created_at: row.get("created_at"),
        duration_ms: row.get("duration_ms"),
        source: row.get("source"),
        language: row.get("language"),
    }
}

pub async fn set_meeting_duration(pool: &SqlitePool, id: &str, duration_ms: i64) -> Result<()> {
    sqlx::query("UPDATE meetings SET duration_ms = ? WHERE id = ?")
        .bind(duration_ms)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn rename_meeting(pool: &SqlitePool, id: &str, title: &str) -> Result<()> {
    sqlx::query("UPDATE meetings SET title = ? WHERE id = ?")
        .bind(title)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_meeting(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM meetings WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Lists meetings newest-first; with `query`, filters by title or transcript
/// content and attaches a matching snippet.
pub async fn list_meetings(pool: &SqlitePool, query: Option<&str>) -> Result<Vec<MeetingListItem>> {
    let like = query
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .map(|q| format!("%{}%", q.replace('%', "\\%").replace('_', "\\_")));

    let rows = match &like {
        Some(pattern) => {
            sqlx::query(
                r#"
                SELECT m.id, m.title, m.created_at, m.duration_ms, m.source, m.language,
                       (SELECT COUNT(*) FROM segments s WHERE s.meeting_id = m.id) AS segment_count,
                       EXISTS(SELECT 1 FROM summaries su WHERE su.meeting_id = m.id) AS has_summary,
                       (SELECT s.text FROM segments s
                         WHERE s.meeting_id = m.id AND s.text LIKE ? ESCAPE '\'
                         ORDER BY s.start_ms LIMIT 1) AS snippet
                FROM meetings m
                WHERE m.title LIKE ? ESCAPE '\'
                   OR EXISTS(SELECT 1 FROM segments s
                             WHERE s.meeting_id = m.id AND s.text LIKE ? ESCAPE '\')
                ORDER BY m.created_at DESC
                "#,
            )
            .bind(pattern)
            .bind(pattern)
            .bind(pattern)
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query(
                r#"
                SELECT m.id, m.title, m.created_at, m.duration_ms, m.source, m.language,
                       (SELECT COUNT(*) FROM segments s WHERE s.meeting_id = m.id) AS segment_count,
                       EXISTS(SELECT 1 FROM summaries su WHERE su.meeting_id = m.id) AS has_summary,
                       NULL AS snippet
                FROM meetings m
                ORDER BY m.created_at DESC
                "#,
            )
            .fetch_all(pool)
            .await?
        }
    };

    Ok(rows
        .iter()
        .map(|row| MeetingListItem {
            meeting: meeting_from_row(row),
            segment_count: row.get("segment_count"),
            has_summary: row.get::<i64, _>("has_summary") != 0,
            snippet: row.get("snippet"),
        })
        .collect())
}

// ---- segments ----

fn segment_from_row(row: &sqlx::sqlite::SqliteRow) -> Segment {
    Segment {
        id: row.get("id"),
        meeting_id: row.get("meeting_id"),
        start_ms: row.get("start_ms"),
        end_ms: row.get("end_ms"),
        text: row.get("text"),
        speaker: row.get("speaker"),
    }
}

pub async fn insert_segment(
    pool: &SqlitePool,
    meeting_id: &str,
    start_ms: i64,
    end_ms: i64,
    text: &str,
    speaker: Option<i64>,
) -> Result<Segment> {
    let row = sqlx::query(
        "INSERT INTO segments (meeting_id, start_ms, end_ms, text, speaker) VALUES (?, ?, ?, ?, ?)
         RETURNING id, meeting_id, start_ms, end_ms, text, speaker",
    )
    .bind(meeting_id)
    .bind(start_ms)
    .bind(end_ms)
    .bind(text)
    .bind(speaker)
    .fetch_one(pool)
    .await?;
    Ok(segment_from_row(&row))
}

pub async fn list_segments(pool: &SqlitePool, meeting_id: &str) -> Result<Vec<Segment>> {
    let rows = sqlx::query(
        "SELECT id, meeting_id, start_ms, end_ms, text, speaker FROM segments
         WHERE meeting_id = ? ORDER BY start_ms",
    )
    .bind(meeting_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(segment_from_row).collect())
}

// ---- summaries ----

#[allow(clippy::too_many_arguments)]
pub async fn insert_summary(
    pool: &SqlitePool,
    meeting_id: &str,
    template_id: &str,
    language: &str,
    content: &str,
    provider: &str,
    model: &str,
) -> Result<Summary> {
    let row = sqlx::query(
        "INSERT INTO summaries (meeting_id, template_id, language, content, provider, model)
         VALUES (?, ?, ?, ?, ?, ?)
         RETURNING id, meeting_id, template_id, language, content, provider, model, created_at",
    )
    .bind(meeting_id)
    .bind(template_id)
    .bind(language)
    .bind(content)
    .bind(provider)
    .bind(model)
    .fetch_one(pool)
    .await?;
    Ok(summary_from_row(&row))
}

fn summary_from_row(row: &sqlx::sqlite::SqliteRow) -> Summary {
    Summary {
        id: row.get("id"),
        meeting_id: row.get("meeting_id"),
        template_id: row.get("template_id"),
        language: row.get("language"),
        content: row.get("content"),
        provider: row.get("provider"),
        model: row.get("model"),
        created_at: row.get("created_at"),
    }
}

pub async fn list_summaries(pool: &SqlitePool, meeting_id: &str) -> Result<Vec<Summary>> {
    let rows = sqlx::query(
        "SELECT id, meeting_id, template_id, language, content, provider, model, created_at
         FROM summaries WHERE meeting_id = ? ORDER BY created_at DESC",
    )
    .bind(meeting_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(summary_from_row).collect())
}

// ---- settings ----

pub async fn get_setting(pool: &SqlitePool, key: &str) -> Result<Option<String>> {
    let row = sqlx::query("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.get("value")))
}

pub async fn set_setting(pool: &SqlitePool, key: &str, value: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> SqlitePool {
        let dir = tempfile::tempdir().unwrap();
        // Keep the tempdir alive for the duration of the pool by leaking it;
        // tests are short-lived processes.
        let path = Box::leak(Box::new(dir)).path().join("test.db");
        open(&path).await.unwrap()
    }

    #[tokio::test]
    async fn meeting_crud_roundtrip() {
        let pool = test_pool().await;
        let m = create_meeting(&pool, "m1", "Standup", "live", Some("en"))
            .await
            .unwrap();
        assert_eq!(m.title, "Standup");

        rename_meeting(&pool, "m1", "Daily standup").await.unwrap();
        set_meeting_duration(&pool, "m1", 65_000).await.unwrap();
        let m = get_meeting(&pool, "m1").await.unwrap();
        assert_eq!(m.title, "Daily standup");
        assert_eq!(m.duration_ms, 65_000);

        delete_meeting(&pool, "m1").await.unwrap();
        assert!(get_meeting(&pool, "m1").await.is_err());
    }

    #[tokio::test]
    async fn segments_are_ordered_and_cascade_deleted() {
        let pool = test_pool().await;
        create_meeting(&pool, "m1", "T", "live", None)
            .await
            .unwrap();
        insert_segment(&pool, "m1", 5000, 6000, "second", None)
            .await
            .unwrap();
        insert_segment(&pool, "m1", 0, 1000, "first", Some(1))
            .await
            .unwrap();

        let segs = list_segments(&pool, "m1").await.unwrap();
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].text, "first");
        assert_eq!(segs[0].speaker, Some(1));
        assert_eq!(segs[1].speaker, None);

        delete_meeting(&pool, "m1").await.unwrap();
        assert!(list_segments(&pool, "m1").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn search_matches_title_and_transcript_with_snippet() {
        let pool = test_pool().await;
        create_meeting(&pool, "m1", "Budget review", "live", None)
            .await
            .unwrap();
        create_meeting(&pool, "m2", "Standup", "live", None)
            .await
            .unwrap();
        insert_segment(
            &pool,
            "m2",
            0,
            1000,
            "we discussed the budget overrun",
            None,
        )
        .await
        .unwrap();
        create_meeting(&pool, "m3", "1:1", "live", None)
            .await
            .unwrap();

        let hits = list_meetings(&pool, Some("budget")).await.unwrap();
        assert_eq!(hits.len(), 2);
        let m2 = hits.iter().find(|h| h.meeting.id == "m2").unwrap();
        assert_eq!(
            m2.snippet.as_deref(),
            Some("we discussed the budget overrun")
        );

        let all = list_meetings(&pool, None).await.unwrap();
        assert_eq!(all.len(), 3);
    }

    #[tokio::test]
    async fn like_wildcards_in_search_are_escaped() {
        let pool = test_pool().await;
        create_meeting(&pool, "m1", "All hands", "live", None)
            .await
            .unwrap();
        // `%` must not act as match-anything.
        let hits = list_meetings(&pool, Some("%")).await.unwrap();
        assert!(hits.is_empty());
    }

    #[tokio::test]
    async fn summaries_roundtrip() {
        let pool = test_pool().await;
        create_meeting(&pool, "m1", "T", "live", None)
            .await
            .unwrap();
        insert_summary(
            &pool,
            "m1",
            "standard",
            "Polish",
            "## Summary\n…",
            "ollama",
            "llama3.2",
        )
        .await
        .unwrap();
        let sums = list_summaries(&pool, "m1").await.unwrap();
        assert_eq!(sums.len(), 1);
        assert_eq!(sums[0].template_id, "standard");
    }

    #[tokio::test]
    async fn settings_upsert() {
        let pool = test_pool().await;
        assert!(get_setting(&pool, "k").await.unwrap().is_none());
        set_setting(&pool, "k", "v1").await.unwrap();
        set_setting(&pool, "k", "v2").await.unwrap();
        assert_eq!(
            get_setting(&pool, "k").await.unwrap().as_deref(),
            Some("v2")
        );
    }
}
