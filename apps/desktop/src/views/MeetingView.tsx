import { useCallback, useEffect, useState } from "react";
import Markdown from "react-markdown";
import { save } from "@tauri-apps/plugin-dialog";
import { api } from "../lib/api";
import { useStore } from "../lib/store";
import { formatDate, formatDuration, formatTimestamp } from "../lib/format";
import type { MeetingDetail, SummaryTemplate } from "../lib/types";

export function MeetingView({ meetingId }: { meetingId: string }) {
  const navigate = useStore((s) => s.navigate);
  const [detail, setDetail] = useState<MeetingDetail | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(() => {
    api.getMeeting(meetingId).then(setDetail).catch((e) => setError(e.message));
  }, [meetingId]);

  useEffect(reload, [reload]);

  if (error) {
    return (
      <div className="flex h-full items-center justify-center">
        <p className="text-[13px] text-signal-400">{error}</p>
      </div>
    );
  }
  if (!detail) return null;
  const { meeting, segments, summaries } = detail;

  return (
    <div className="view-in flex h-full flex-col">
      <header className="flex items-start justify-between gap-6 border-b border-ink-700/60 px-8 pb-5 pt-7">
        <div className="min-w-0">
          <button
            onClick={() => navigate({ name: "library" })}
            className="font-mono text-[11px] text-mist-500 transition hover:text-mist-300"
          >
            ← Library
          </button>
          <EditableTitle
            title={meeting.title}
            onRename={async (title) => {
              await api.renameMeeting(meeting.id, title);
              reload();
            }}
          />
          <p className="mt-1 font-mono text-[11px] text-mist-500">
            {formatDate(meeting.createdAt)} · {formatDuration(meeting.durationMs)} ·{" "}
            {segments.length} segments{meeting.source === "import" ? " · imported" : ""}
          </p>
        </div>
        <HeaderActions meetingId={meeting.id} title={meeting.title} />
      </header>

      <div className="flex min-h-0 flex-1">
        {/* Transcript column */}
        <section className="selectable min-w-0 flex-1 overflow-y-auto px-8 py-6">
          {segments.length === 0 ? (
            <p className="mt-12 text-center text-[13px] text-mist-500">
              No speech was detected in this meeting.
            </p>
          ) : (
            <div className="mx-auto max-w-2xl space-y-4 pb-16">
              {segments.map((seg) => (
                <div key={seg.id} className="flex gap-4">
                  <span className="mt-[3px] shrink-0 font-mono text-[11px] tabular-nums text-mist-500">
                    {formatTimestamp(seg.startMs)}
                  </span>
                  <p className="text-[14px] leading-relaxed text-mist-100">{seg.text}</p>
                </div>
              ))}
            </div>
          )}
        </section>

        {/* Summary rail */}
        <SummaryRail
          meetingId={meeting.id}
          hasTranscript={segments.length > 0}
          summary={summaries[0] ?? null}
          onGenerated={reload}
        />
      </div>
    </div>
  );
}

function EditableTitle({
  title,
  onRename,
}: {
  title: string;
  onRename: (title: string) => Promise<void>;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(title);

  if (!editing) {
    return (
      <h2
        title="Click to rename"
        onClick={() => {
          setDraft(title);
          setEditing(true);
        }}
        className="mt-1 cursor-text truncate font-display text-xl font-semibold tracking-tight text-paper"
      >
        {title}
      </h2>
    );
  }
  const commit = async () => {
    setEditing(false);
    const next = draft.trim();
    if (next && next !== title) await onRename(next);
  };
  return (
    <input
      autoFocus
      value={draft}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={() => void commit()}
      onKeyDown={(e) => {
        if (e.key === "Enter") void commit();
        if (e.key === "Escape") setEditing(false);
      }}
      className="mt-1 w-full max-w-lg rounded border border-tide-500 bg-ink-850 px-2 py-1 font-display text-xl font-semibold tracking-tight text-paper focus:outline-none"
    />
  );
}

function HeaderActions({ meetingId, title }: { meetingId: string; title: string }) {
  const navigate = useStore((s) => s.navigate);
  const [copied, setCopied] = useState(false);
  const [confirming, setConfirming] = useState(false);

  const onCopy = async () => {
    const markdown = await api.exportMarkdown(meetingId);
    await navigator.clipboard.writeText(markdown);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  const onSave = async () => {
    const path = await save({
      title: "Export as Markdown",
      defaultPath: `${title.replace(/[/\\:*?"<>|]/g, "-")}.md`,
      filters: [{ name: "Markdown", extensions: ["md"] }],
    });
    if (path) await api.saveMarkdown(meetingId, path);
  };

  const onDelete = async () => {
    await api.deleteMeeting(meetingId);
    navigate({ name: "library" });
  };

  return (
    <div className="flex shrink-0 items-center gap-2 pt-5">
      <ActionButton onClick={() => void onCopy()}>
        {copied ? "Copied ✓" : "Copy Markdown"}
      </ActionButton>
      <ActionButton onClick={() => void onSave()}>Export .md</ActionButton>
      {confirming ? (
        <ActionButton tone="danger" onClick={() => void onDelete()}>
          Confirm delete
        </ActionButton>
      ) : (
        <ActionButton tone="danger" onClick={() => setConfirming(true)}>
          Delete
        </ActionButton>
      )}
    </div>
  );
}

function ActionButton({
  children,
  onClick,
  tone,
}: {
  children: React.ReactNode;
  onClick: () => void;
  tone?: "danger";
}) {
  return (
    <button
      onClick={onClick}
      className={`rounded-md border px-3 py-1.5 text-[12px] transition ${
        tone === "danger"
          ? "border-signal-500/30 text-signal-400 hover:bg-signal-500/10"
          : "border-ink-600 text-mist-100 hover:border-mist-500 hover:bg-ink-800"
      }`}
    >
      {children}
    </button>
  );
}

/* ---------- summary rail ---------- */

function SummaryRail({
  meetingId,
  hasTranscript,
  summary,
  onGenerated,
}: {
  meetingId: string;
  hasTranscript: boolean;
  summary: import("../lib/types").Summary | null;
  onGenerated: () => void;
}) {
  const [templates, setTemplates] = useState<SummaryTemplate[]>([]);
  const [templateId, setTemplateId] = useState<string | null>(null);
  const [generating, setGenerating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void api.listTemplates().then(setTemplates).catch(() => {});
  }, []);

  const onGenerate = async () => {
    setGenerating(true);
    setError(null);
    try {
      await api.generateSummary(meetingId, templateId ?? undefined);
      onGenerated();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setGenerating(false);
    }
  };

  return (
    <aside className="flex w-[340px] shrink-0 flex-col border-l border-ink-700/60 bg-ink-950/40">
      <div className="flex items-center justify-between border-b border-ink-700/60 px-5 py-4">
        <h3 className="font-display text-[12px] font-semibold uppercase tracking-[0.15em] text-mist-300">
          AI summary
        </h3>
        {summary && (
          <span className="font-mono text-[10px] text-mist-500">
            {summary.provider} · {summary.model}
          </span>
        )}
      </div>

      <div className="selectable min-h-0 flex-1 overflow-y-auto px-5 py-4">
        {summary ? (
          <div className="summary-md text-[13px] text-mist-100">
            <Markdown>{summary.content}</Markdown>
          </div>
        ) : (
          <p className="mt-6 text-[12.5px] leading-relaxed text-mist-500">
            {hasTranscript
              ? "Generate structured notes — key points, decisions and action items — from this transcript. Text is sent only to the AI provider you configured; never the audio."
              : "Once this meeting has a transcript you can generate structured notes here."}
          </p>
        )}
        {error && (
          <p className="mt-3 rounded-md border border-signal-500/30 bg-signal-500/10 px-3 py-2 text-[12px] text-signal-400">
            {error}
          </p>
        )}
      </div>

      {hasTranscript && (
        <div className="border-t border-ink-700/60 px-5 py-4">
          <select
            value={templateId ?? ""}
            onChange={(e) => setTemplateId(e.target.value || null)}
            className="mb-3 w-full rounded-md border border-ink-600 bg-ink-850 px-2.5 py-2 text-[12.5px] text-mist-100 focus:border-tide-500 focus:outline-none"
          >
            <option value="">Default template</option>
            {templates.map((t) => (
              <option key={t.id} value={t.id}>
                {t.name}
              </option>
            ))}
          </select>
          <button
            onClick={() => void onGenerate()}
            disabled={generating}
            className="w-full rounded-md bg-tide-500 px-4 py-2.5 font-display text-[13px] font-semibold text-ink-950 transition hover:bg-tide-400 disabled:cursor-wait disabled:opacity-50"
          >
            {generating ? "Summarizing…" : summary ? "Regenerate" : "Generate summary"}
          </button>
        </div>
      )}
    </aside>
  );
}
