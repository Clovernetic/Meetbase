import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../lib/api";
import { useStore } from "../lib/store";
import { formatClock, formatTimestamp } from "../lib/format";
import type { Segment } from "../lib/types";

export function RecordView() {
  const recording = useStore((s) => s.recording);
  return recording.meetingId ? <LiveSession /> : <IdleDeck />;
}

/* ---------- idle: ready to record ---------- */

function IdleDeck() {
  const startRecording = useStore((s) => s.startRecording);
  const navigate = useStore((s) => s.navigate);
  const [title, setTitle] = useState("");
  const [busy, setBusy] = useState<"record" | "import" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [modelReady, setModelReady] = useState<boolean | null>(null);

  useEffect(() => {
    void (async () => {
      try {
        const [settings, models] = await Promise.all([
          api.getSettings(),
          api.listModels(),
        ]);
        setModelReady(
          models.some((m) => m.id === settings.whisperModel && m.downloaded),
        );
      } catch {
        setModelReady(null);
      }
    })();
  }, []);

  const onRecord = async () => {
    setBusy("record");
    setError(null);
    try {
      await startRecording(title.trim() || undefined);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  };

  const onImport = async () => {
    const path = await open({
      multiple: false,
      title: "Import audio or video",
      filters: [
        {
          name: "Media",
          extensions: ["wav", "mp3", "m4a", "aac", "ogg", "flac", "mp4", "mov", "webm"],
        },
      ],
    });
    if (typeof path !== "string") return;
    setBusy("import");
    setError(null);
    try {
      const meeting = await api.importMedia(path);
      navigate({ name: "meeting", id: meeting.id });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="view-in flex h-full flex-col items-center justify-center px-8">
      <div className="w-full max-w-md">
        <p className="font-mono text-[11px] uppercase tracking-[0.2em] text-tide-400">
          New session
        </p>
        <h2 className="mt-2 font-display text-3xl font-semibold tracking-tight text-paper">
          Ready when you are.
        </h2>
        <p className="mt-2 text-[13px] leading-relaxed text-mist-300">
          Audio is captured, transcribed and stored on this machine only.
        </p>

        <input
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && !busy && void onRecord()}
          placeholder="Meeting title (optional)"
          className="mt-8 w-full rounded-md border border-ink-600 bg-ink-850 px-3.5 py-2.5 text-[14px] text-paper placeholder:text-mist-500 focus:border-tide-500 focus:outline-none"
        />

        <div className="mt-4 flex items-center gap-3">
          <button
            onClick={() => void onRecord()}
            disabled={busy !== null || modelReady === false}
            className="flex flex-1 items-center justify-center gap-2.5 rounded-md bg-signal-500 px-5 py-3 font-display text-[14px] font-semibold text-ink-950 transition hover:bg-signal-400 disabled:cursor-not-allowed disabled:opacity-40"
          >
            <span className="h-2.5 w-2.5 rounded-full bg-ink-950/80" />
            {busy === "record" ? "Starting…" : "Start recording"}
          </button>
          <button
            onClick={() => void onImport()}
            disabled={busy !== null || modelReady === false}
            className="rounded-md border border-ink-600 px-4 py-3 text-[13px] text-mist-100 transition hover:border-mist-500 hover:bg-ink-800 disabled:cursor-not-allowed disabled:opacity-40"
          >
            {busy === "import" ? "Transcribing…" : "Import file"}
          </button>
        </div>

        {modelReady === false && (
          <Notice tone="warn">
            No speech model downloaded yet.{" "}
            <button
              className="underline decoration-amber-glow/50 underline-offset-2 hover:text-paper"
              onClick={() => useStore.getState().navigate({ name: "settings" })}
            >
              Download one in Settings
            </button>{" "}
            to start transcribing.
          </Notice>
        )}
        {error && <Notice tone="error">{error}</Notice>}
        {busy === "import" && (
          <Notice tone="info">
            Transcribing the file locally — this can take a few minutes for long
            recordings.
          </Notice>
        )}
      </div>
    </div>
  );
}

function Notice({
  tone,
  children,
}: {
  tone: "warn" | "error" | "info";
  children: React.ReactNode;
}) {
  const styles = {
    warn: "border-amber-glow/30 bg-amber-glow/10 text-amber-glow",
    error: "border-signal-500/30 bg-signal-500/10 text-signal-400",
    info: "border-tide-500/30 bg-tide-500/10 text-tide-300",
  }[tone];
  return (
    <div className={`mt-4 rounded-md border px-3.5 py-2.5 text-[12.5px] leading-relaxed ${styles}`}>
      {children}
    </div>
  );
}

/* ---------- live recording session ---------- */

function LiveSession() {
  const recording = useStore((s) => s.recording);
  const stopRecording = useStore((s) => s.stopRecording);
  const navigate = useStore((s) => s.navigate);
  const [error, setError] = useState<string | null>(null);

  const onStop = async () => {
    setError(null);
    try {
      const meetingId = await stopRecording();
      if (meetingId) navigate({ name: "meeting", id: meetingId });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="view-in flex h-full flex-col">
      {/* Console header */}
      <header className="flex items-center justify-between border-b border-ink-700/60 px-8 py-5">
        <div className="flex items-center gap-4">
          <span className="rec-pulse h-3 w-3 rounded-full bg-signal-500" />
          <div>
            <p className="font-mono text-[10.5px] uppercase tracking-[0.2em] text-signal-400">
              Recording
            </p>
            <LiveTimer startedAt={recording.startedAt} />
          </div>
        </div>

        <div className="flex items-center gap-5">
          <LevelMeter />
          <button
            onClick={() => void onStop()}
            disabled={recording.stopping}
            className="rounded-md border border-signal-500/50 bg-signal-500/10 px-5 py-2.5 font-display text-[13px] font-semibold text-signal-400 transition hover:bg-signal-500 hover:text-ink-950 disabled:cursor-wait disabled:opacity-50"
          >
            {recording.stopping ? "Finishing transcript…" : "Stop & save"}
          </button>
        </div>
      </header>

      {error && (
        <div className="mx-8 mt-4 rounded-md border border-signal-500/30 bg-signal-500/10 px-3.5 py-2.5 text-[12.5px] text-signal-400">
          {error}
        </div>
      )}

      <LiveTranscript segments={recording.liveSegments} />
    </div>
  );
}

function LiveTimer({ startedAt }: { startedAt: number | null }) {
  const [, tick] = useState(0);
  useEffect(() => {
    const id = setInterval(() => tick((n) => n + 1), 250);
    return () => clearInterval(id);
  }, []);
  return (
    <p className="font-mono text-2xl font-medium tabular-nums tracking-tight text-paper">
      {startedAt ? formatClock(Date.now() - startedAt) : "00:00"}
    </p>
  );
}

/** Decorative input-level bars; honest about being ambience, not telemetry. */
function LevelMeter() {
  const heights = [14, 22, 30, 24, 18, 26, 32, 20, 12];
  return (
    <div aria-hidden className="flex h-8 items-center gap-[3px]">
      {heights.map((h, i) => (
        <span
          key={i}
          className="level-bar w-[3px] rounded-full bg-tide-400/80"
          style={{ height: `${h}px`, animationDelay: `${i * 110}ms` }}
        />
      ))}
    </div>
  );
}

function LiveTranscript({ segments }: { segments: Segment[] }) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);

  useEffect(() => {
    const el = scrollRef.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  }, [segments.length]);

  return (
    <div
      ref={scrollRef}
      onScroll={(e) => {
        const el = e.currentTarget;
        pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 60;
      }}
      className="selectable flex-1 overflow-y-auto px-8 py-6"
    >
      {segments.length === 0 ? (
        <p className="mt-16 text-center text-[13px] text-mist-500">
          Listening… the transcript will appear here as people speak.
        </p>
      ) : (
        <div className="mx-auto max-w-2xl space-y-4 pb-24">
          {segments.map((seg) => (
            <div key={seg.id} className="segment-in flex gap-4">
              <span className="mt-[3px] shrink-0 font-mono text-[11px] tabular-nums text-mist-500">
                {formatTimestamp(seg.startMs)}
              </span>
              <p className="text-[14.5px] leading-relaxed text-mist-100">{seg.text}</p>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
