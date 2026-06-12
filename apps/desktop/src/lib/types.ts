/** Mirrors of the Rust backend's serialized types (see src-tauri). */

export interface Meeting {
  id: string;
  title: string;
  createdAt: string;
  durationMs: number;
  source: "live" | "import";
  language: string | null;
}

export interface MeetingListItem extends Meeting {
  segmentCount: number;
  hasSummary: boolean;
  snippet: string | null;
}

export interface Segment {
  id: number;
  meetingId: string;
  startMs: number;
  endMs: number;
  text: string;
  /** 1-based speaker id from diarization; null when unknown/disabled. */
  speaker: number | null;
}

export interface Summary {
  id: number;
  meetingId: string;
  templateId: string;
  language: string;
  content: string;
  provider: string;
  model: string;
  createdAt: string;
}

export interface MeetingDetail {
  meeting: Meeting;
  segments: Segment[];
  summaries: Summary[];
}

export interface ModelEntry {
  id: string;
  displayName: string;
  sizeBytes: number;
  qualityHint: string;
  downloaded: boolean;
}

export type ProviderConfig =
  | { kind: "ollama"; baseUrl: string; model: string }
  | { kind: "open_ai_compat"; baseUrl: string; apiKey: string; model: string };

export interface AppSettings {
  whisperModel: string;
  spokenLanguage: string | null;
  summaryLanguage: string;
  summaryTemplate: string;
  llm: ProviderConfig | null;
  micDevice: string | null;
  captureSystemAudio: boolean;
  diarization: boolean;
}

export interface SummaryTemplate {
  id: string;
  name: string;
  instructions: string;
}

export interface RecordingStatus {
  meetingId: string | null;
  elapsedMs: number;
}

export interface StopResult {
  meetingId: string;
  durationMs: number;
}

// ---- event payloads ----

export interface SegmentEvent {
  meetingId: string;
  segment: Segment;
}

export interface DownloadProgressEvent {
  modelId: string;
  downloaded: number;
  total: number;
}
