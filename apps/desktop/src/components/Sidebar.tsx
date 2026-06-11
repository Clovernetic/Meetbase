import { useStore } from "../lib/store";
import { formatClock } from "../lib/format";
import { useEffect, useState } from "react";

const NAV = [
  { key: "record", label: "Record", glyph: "●" },
  { key: "library", label: "Library", glyph: "≡" },
  { key: "settings", label: "Settings", glyph: "⚙" },
] as const;

export function Sidebar() {
  const view = useStore((s) => s.view);
  const navigate = useStore((s) => s.navigate);
  const recording = useStore((s) => s.recording);

  const activeKey =
    view.name === "meeting" ? "library" : view.name === "record" ? "record" : view.name;

  return (
    <aside className="relative z-10 flex w-52 shrink-0 flex-col border-r border-ink-700/60 bg-ink-950/70">
      {/* Wordmark */}
      <div className="px-5 pt-6 pb-7">
        <h1 className="font-display text-[15px] font-semibold tracking-tight text-paper">
          meet<span className="text-tide-400">base</span>
        </h1>
        <p className="mt-1 text-[10.5px] leading-snug text-mist-500">
          Meetings stay on this machine.
        </p>
      </div>

      <nav className="flex flex-col gap-0.5 px-3">
        {NAV.map((item) => {
          const active = activeKey === item.key;
          return (
            <button
              key={item.key}
              onClick={() => navigate({ name: item.key })}
              className={`group flex items-center gap-3 rounded-md px-3 py-2 text-left text-[13px] transition-colors ${
                active
                  ? "bg-ink-700/70 text-paper"
                  : "text-mist-300 hover:bg-ink-800 hover:text-mist-100"
              }`}
            >
              <span
                className={`w-4 text-center text-[11px] ${
                  active ? "text-tide-400" : "text-mist-500 group-hover:text-mist-300"
                }`}
              >
                {item.glyph}
              </span>
              {item.label}
            </button>
          );
        })}
      </nav>

      <div className="flex-1" />

      {/* Live recording indicator, always visible */}
      {recording.meetingId && (
        <button
          onClick={() => navigate({ name: "record" })}
          className="mx-3 mb-3 flex items-center gap-2.5 rounded-md border border-signal-500/30 bg-signal-500/10 px-3 py-2.5 text-left"
        >
          <span className="rec-pulse h-2 w-2 shrink-0 rounded-full bg-signal-500" />
          <RecordingTimer startedAt={recording.startedAt} />
        </button>
      )}

      <div className="border-t border-ink-700/60 px-5 py-3.5">
        <p className="font-mono text-[10px] tracking-wide text-mist-500">
          100% local · MIT
        </p>
      </div>
    </aside>
  );
}

function RecordingTimer({ startedAt }: { startedAt: number | null }) {
  const [, tick] = useState(0);
  useEffect(() => {
    const id = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(id);
  }, []);
  return (
    <span className="font-mono text-[12px] font-medium tabular-nums text-signal-400">
      {startedAt ? formatClock(Date.now() - startedAt) : "00:00"}
    </span>
  );
}
