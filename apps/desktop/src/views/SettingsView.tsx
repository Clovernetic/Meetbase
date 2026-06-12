import { useEffect, useRef, useState } from "react";
import { api, events } from "../lib/api";
import { formatBytes } from "../lib/format";
import type { AppSettings, ModelEntry, ProviderConfig } from "../lib/types";

const LANGUAGES: Array<{ code: string | null; label: string }> = [
  { code: null, label: "Auto-detect" },
  { code: "en", label: "English" },
  { code: "pl", label: "Polski" },
  { code: "de", label: "Deutsch" },
  { code: "fr", label: "Français" },
  { code: "es", label: "Español" },
  { code: "it", label: "Italiano" },
  { code: "pt", label: "Português" },
  { code: "nl", label: "Nederlands" },
  { code: "uk", label: "Українська" },
  { code: "cs", label: "Čeština" },
];

const SUMMARY_LANGUAGES = [
  "English",
  "Polski",
  "Deutsch",
  "Français",
  "Español",
  "Italiano",
  "Português",
  "Nederlands",
  "Українська",
  "Čeština",
];

export function SettingsView() {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [saved, setSaved] = useState(false);
  const savedTimer = useRef<number | undefined>(undefined);

  useEffect(() => {
    void api.getSettings().then(setSettings);
  }, []);

  const update = (patch: Partial<AppSettings>) => {
    setSettings((prev) => {
      if (!prev) return prev;
      const next = { ...prev, ...patch };
      void api.setSettings(next).then(() => {
        setSaved(true);
        window.clearTimeout(savedTimer.current);
        savedTimer.current = window.setTimeout(() => setSaved(false), 1200);
      });
      return next;
    });
  };

  if (!settings) return null;

  return (
    <div className="view-in h-full overflow-y-auto">
      <div className="mx-auto max-w-2xl px-8 pb-16 pt-7">
        <header className="flex items-baseline justify-between">
          <h2 className="font-display text-xl font-semibold tracking-tight text-paper">
            Settings
          </h2>
          <span
            className={`font-mono text-[11px] text-tide-400 transition-opacity ${
              saved ? "opacity-100" : "opacity-0"
            }`}
          >
            saved ✓
          </span>
        </header>

        <Section
          title="Speech recognition"
          hint="Models run fully offline. Larger models are more accurate but slower."
        >
          <ModelsManager
            activeModel={settings.whisperModel}
            onSelect={(id) => update({ whisperModel: id })}
          />
          <Field label="Spoken language">
            <select
              value={settings.spokenLanguage ?? ""}
              onChange={(e) => update({ spokenLanguage: e.target.value || null })}
              className={selectCls}
            >
              {LANGUAGES.map((l) => (
                <option key={l.code ?? "auto"} value={l.code ?? ""}>
                  {l.label}
                </option>
              ))}
            </select>
          </Field>
        </Section>

        <Section
          title="AI summaries"
          hint="Summaries need a language model. Ollama keeps everything local; an API key sends transcript text (never audio) to that provider."
        >
          <ProviderEditor
            value={settings.llm}
            onChange={(llm) => update({ llm })}
          />
          <Field label="Summary language">
            <select
              value={settings.summaryLanguage}
              onChange={(e) => update({ summaryLanguage: e.target.value })}
              className={selectCls}
            >
              {SUMMARY_LANGUAGES.map((l) => (
                <option key={l}>{l}</option>
              ))}
            </select>
          </Field>
        </Section>

        <Section title="Audio">
          <MicPicker
            value={settings.micDevice}
            onChange={(micDevice) => update({ micDevice })}
          />
          <label className="mt-4 flex cursor-pointer items-center gap-3">
            <input
              type="checkbox"
              checked={settings.captureSystemAudio}
              onChange={(e) => update({ captureSystemAudio: e.target.checked })}
              className="h-4 w-4 accent-tide-500"
            />
            <div>
              <p className="text-[13px] text-mist-100">Capture system audio</p>
              <p className="text-[11.5px] text-mist-500">
                Mixes other participants' voices into the recording. Needs
                macOS 14.4+ (grant the system-audio permission when asked) or
                Windows. If transcripts miss the other side, check System
                Settings → Privacy &amp; Security → Screen &amp; System Audio
                Recording.
              </p>
            </div>
          </label>
        </Section>
      </div>
    </div>
  );
}

const selectCls =
  "w-full rounded-md border border-ink-600 bg-ink-850 px-2.5 py-2 text-[13px] text-mist-100 focus:border-tide-500 focus:outline-none";
const inputCls =
  "w-full rounded-md border border-ink-600 bg-ink-850 px-2.5 py-2 text-[13px] text-paper placeholder:text-mist-500 focus:border-tide-500 focus:outline-none";

function Section({
  title,
  hint,
  children,
}: {
  title: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <section className="mt-9">
      <h3 className="font-display text-[12px] font-semibold uppercase tracking-[0.15em] text-tide-300">
        {title}
      </h3>
      {hint && <p className="mt-1.5 text-[12px] leading-relaxed text-mist-500">{hint}</p>}
      <div className="mt-4 rounded-lg border border-ink-700/70 bg-ink-850/50 p-5">
        {children}
      </div>
    </section>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="mt-4 first:mt-0">
      <label className="mb-1.5 block text-[12px] font-medium text-mist-300">{label}</label>
      {children}
    </div>
  );
}

/* ---------- whisper models ---------- */

function ModelsManager({
  activeModel,
  onSelect,
}: {
  activeModel: string;
  onSelect: (id: string) => void;
}) {
  const [models, setModels] = useState<ModelEntry[]>([]);
  const [progress, setProgress] = useState<Record<string, number>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});

  const refresh = () => void api.listModels().then(setModels).catch(() => {});
  useEffect(refresh, []);

  useEffect(() => {
    const unlisten = events.onModelDownloadProgress((e) => {
      setProgress((p) => ({ ...p, [e.modelId]: e.total ? e.downloaded / e.total : 0 }));
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  const download = async (id: string) => {
    setErrors((e) => ({ ...e, [id]: "" }));
    setProgress((p) => ({ ...p, [id]: 0 }));
    try {
      await api.downloadModel(id);
    } catch (e) {
      setErrors((errs) => ({ ...errs, [id]: e instanceof Error ? e.message : String(e) }));
    } finally {
      setProgress((p) => {
        const { [id]: _, ...rest } = p;
        return rest;
      });
      refresh();
    }
  };

  return (
    <ul className="space-y-1.5">
      {models.map((m) => {
        const downloading = m.id in progress;
        const active = m.id === activeModel;
        return (
          <li
            key={m.id}
            className={`rounded-md border px-3.5 py-2.5 transition ${
              active ? "border-tide-500/50 bg-tide-500/5" : "border-ink-700/70"
            }`}
          >
            <div className="flex items-center gap-3">
              <div className="min-w-0 flex-1">
                <p className="text-[13px] font-medium text-mist-100">
                  {m.displayName}
                  <span className="ml-2 font-mono text-[10.5px] text-mist-500">
                    {formatBytes(m.sizeBytes)}
                  </span>
                </p>
                <p className="mt-0.5 truncate text-[11px] text-mist-500" title={m.qualityHint}>
                  {m.qualityHint}
                </p>
              </div>
              {m.downloaded ? (
                active ? (
                  <span className="shrink-0 font-mono text-[10.5px] uppercase tracking-wider text-tide-300">
                    in use
                  </span>
                ) : (
                  <button onClick={() => onSelect(m.id)} className={smallBtnCls}>
                    Use
                  </button>
                )
              ) : downloading ? (
                <span className="shrink-0 font-mono text-[11px] tabular-nums text-tide-300">
                  {Math.round((progress[m.id] ?? 0) * 100)}%
                </span>
              ) : (
                <button onClick={() => void download(m.id)} className={smallBtnCls}>
                  Download
                </button>
              )}
            </div>
            {downloading && (
              <div className="mt-2 h-1 overflow-hidden rounded-full bg-ink-700">
                <div
                  className="h-full rounded-full bg-tide-400 transition-[width] duration-200"
                  style={{ width: `${(progress[m.id] ?? 0) * 100}%` }}
                />
              </div>
            )}
            {errors[m.id] && (
              <p className="mt-1.5 text-[11.5px] text-signal-400">{errors[m.id]}</p>
            )}
          </li>
        );
      })}
    </ul>
  );
}

const smallBtnCls =
  "shrink-0 rounded border border-ink-600 px-2.5 py-1 text-[11.5px] text-mist-100 transition hover:border-tide-500 hover:text-tide-300";

/* ---------- LLM provider ---------- */

function ProviderEditor({
  value,
  onChange,
}: {
  value: ProviderConfig | null;
  onChange: (config: ProviderConfig | null) => void;
}) {
  const kind = value?.kind ?? "none";
  const [ollamaModels, setOllamaModels] = useState<string[] | null>(null);
  const [ollamaError, setOllamaError] = useState<string | null>(null);

  const ollamaUrl = value?.kind === "ollama" ? value.baseUrl : "http://localhost:11434";

  useEffect(() => {
    if (kind !== "ollama") return;
    setOllamaModels(null);
    setOllamaError(null);
    const handle = setTimeout(() => {
      api
        .listOllamaModels(ollamaUrl)
        .then(setOllamaModels)
        .catch((e) => setOllamaError(e.message));
    }, 300);
    return () => clearTimeout(handle);
  }, [kind, ollamaUrl]);

  return (
    <div>
      <div className="flex gap-1.5">
        {(
          [
            ["none", "Off"],
            ["ollama", "Ollama (local)"],
            ["open_ai_compat", "API key"],
          ] as const
        ).map(([k, label]) => (
          <button
            key={k}
            onClick={() => {
              if (k === "none") onChange(null);
              else if (k === "ollama")
                onChange({ kind: "ollama", baseUrl: "http://localhost:11434", model: "" });
              else
                onChange({
                  kind: "open_ai_compat",
                  baseUrl: "https://api.openai.com/v1",
                  apiKey: "",
                  model: "",
                });
            }}
            className={`rounded-md px-3 py-1.5 text-[12px] transition ${
              kind === k
                ? "bg-tide-500/15 text-tide-300 ring-1 ring-tide-500/40"
                : "text-mist-300 hover:bg-ink-800"
            }`}
          >
            {label}
          </button>
        ))}
      </div>

      {value?.kind === "ollama" && (
        <div className="mt-4">
          <Field label="Server URL">
            <input
              value={value.baseUrl}
              onChange={(e) => onChange({ ...value, baseUrl: e.target.value })}
              className={inputCls}
            />
          </Field>
          <Field label="Model">
            {ollamaModels && ollamaModels.length > 0 ? (
              <select
                value={value.model}
                onChange={(e) => onChange({ ...value, model: e.target.value })}
                className={selectCls}
              >
                <option value="">Choose a model…</option>
                {ollamaModels.map((m) => (
                  <option key={m}>{m}</option>
                ))}
              </select>
            ) : (
              <input
                value={value.model}
                onChange={(e) => onChange({ ...value, model: e.target.value })}
                placeholder="e.g. llama3.2"
                className={inputCls}
              />
            )}
          </Field>
          {ollamaError && (
            <p className="mt-2 text-[11.5px] text-amber-glow">
              Could not reach Ollama at {value.baseUrl} — is it running?
            </p>
          )}
        </div>
      )}

      {value?.kind === "open_ai_compat" && (
        <div className="mt-4">
          <Field label="Base URL">
            <input
              value={value.baseUrl}
              onChange={(e) => onChange({ ...value, baseUrl: e.target.value })}
              placeholder="https://api.openai.com/v1"
              className={inputCls}
            />
          </Field>
          <p className="mt-1.5 text-[11px] text-mist-500">
            Works with OpenAI, Groq, OpenRouter, Anthropic (compat) and any
            OpenAI-compatible endpoint.
          </p>
          <Field label="API key">
            <input
              type="password"
              value={value.apiKey}
              onChange={(e) => onChange({ ...value, apiKey: e.target.value })}
              placeholder="sk-…"
              className={inputCls}
            />
          </Field>
          <Field label="Model">
            <input
              value={value.model}
              onChange={(e) => onChange({ ...value, model: e.target.value })}
              placeholder="e.g. claude-haiku-4-5 or gpt-4.1-mini"
              className={inputCls}
            />
          </Field>
        </div>
      )}
    </div>
  );
}

/* ---------- microphone ---------- */

function MicPicker({
  value,
  onChange,
}: {
  value: string | null;
  onChange: (device: string | null) => void;
}) {
  const [devices, setDevices] = useState<string[]>([]);
  useEffect(() => {
    void api.listAudioDevices().then(setDevices).catch(() => {});
  }, []);

  return (
    <Field label="Microphone">
      <select
        value={value ?? ""}
        onChange={(e) => onChange(e.target.value || null)}
        className={selectCls}
      >
        <option value="">System default</option>
        {devices.map((d) => (
          <option key={d}>{d}</option>
        ))}
      </select>
    </Field>
  );
}
