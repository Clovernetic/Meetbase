import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { useStore } from "../lib/store";
import { formatDate, formatDuration, formatTime } from "../lib/format";
import type { MeetingListItem } from "../lib/types";

export function LibraryView() {
  const navigate = useStore((s) => s.navigate);
  const [query, setQuery] = useState("");
  const [items, setItems] = useState<MeetingListItem[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const handle = setTimeout(
      () => {
        api
          .listMeetings(query.trim() || undefined)
          .then((result) => !cancelled && setItems(result))
          .catch((e) => !cancelled && setError(e.message));
      },
      query ? 200 : 0,
    );
    return () => {
      cancelled = true;
      clearTimeout(handle);
    };
  }, [query]);

  return (
    <div className="view-in flex h-full flex-col">
      <header className="border-b border-ink-700/60 px-8 pb-5 pt-7">
        <h2 className="font-display text-xl font-semibold tracking-tight text-paper">
          Library
        </h2>
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search titles and transcripts…"
          className="mt-4 w-full max-w-md rounded-md border border-ink-600 bg-ink-850 px-3.5 py-2 text-[13px] text-paper placeholder:text-mist-500 focus:border-tide-500 focus:outline-none"
        />
      </header>

      <div className="flex-1 overflow-y-auto px-8 py-5">
        {error && <p className="text-[13px] text-signal-400">{error}</p>}
        {items && items.length === 0 && (
          <p className="mt-12 text-center text-[13px] text-mist-500">
            {query
              ? "Nothing matches that search."
              : "No meetings yet — your first recording will land here."}
          </p>
        )}
        <ul className="mx-auto max-w-3xl space-y-2">
          {items?.map((item, i) => (
            <li key={item.id} style={{ animationDelay: `${Math.min(i, 12) * 30}ms` }} className="segment-in">
              <button
                onClick={() => navigate({ name: "meeting", id: item.id })}
                className="group flex w-full items-baseline gap-4 rounded-lg border border-transparent px-4 py-3.5 text-left transition hover:border-ink-600 hover:bg-ink-850"
              >
                <div className="w-24 shrink-0">
                  <p className="font-mono text-[11px] text-mist-500">
                    {formatDate(item.createdAt)}
                  </p>
                  <p className="font-mono text-[10.5px] text-mist-500/70">
                    {formatTime(item.createdAt)}
                  </p>
                </div>
                <div className="min-w-0 flex-1">
                  <p className="truncate text-[14px] font-medium text-mist-100 group-hover:text-paper">
                    {item.title}
                  </p>
                  {item.snippet && (
                    <p className="mt-0.5 truncate text-[12px] text-mist-500">
                      “{item.snippet}”
                    </p>
                  )}
                </div>
                <div className="flex shrink-0 items-center gap-2.5">
                  {item.source === "import" && <Tag>import</Tag>}
                  {item.hasSummary && <Tag tone="tide">summary</Tag>}
                  <span className="w-14 text-right font-mono text-[11px] tabular-nums text-mist-500">
                    {formatDuration(item.durationMs)}
                  </span>
                </div>
              </button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

function Tag({ children, tone }: { children: React.ReactNode; tone?: "tide" }) {
  return (
    <span
      className={`rounded px-1.5 py-0.5 font-mono text-[9.5px] uppercase tracking-wider ${
        tone === "tide"
          ? "bg-tide-500/15 text-tide-300"
          : "bg-ink-700 text-mist-300"
      }`}
    >
      {children}
    </span>
  );
}
