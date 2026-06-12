/** Typed bindings for the Tauri command surface. */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppSettings,
  DownloadProgressEvent,
  Meeting,
  MeetingDetail,
  MeetingListItem,
  ModelEntry,
  RecordingStatus,
  SegmentEvent,
  StopResult,
  Summary,
  SummaryTemplate,
} from "./types";

/** Backend errors arrive as `{ message }`; normalize to Error instances. */
async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (raw) {
    const message =
      typeof raw === "object" && raw !== null && "message" in raw
        ? String((raw as { message: unknown }).message)
        : String(raw);
    throw new Error(message);
  }
}

export const api = {
  // meetings
  listMeetings: (query?: string) =>
    call<MeetingListItem[]>("list_meetings", { query: query || null }),
  getMeeting: (id: string) => call<MeetingDetail>("get_meeting", { id }),
  renameMeeting: (id: string, title: string) =>
    call<void>("rename_meeting", { id, title }),
  deleteMeeting: (id: string) => call<void>("delete_meeting", { id }),

  // recording
  startRecording: (title?: string) =>
    call<Meeting>("start_recording", { title: title || null }),
  stopRecording: () => call<StopResult>("stop_recording"),
  recordingStatus: () => call<RecordingStatus>("recording_status"),

  // import
  importMedia: (path: string, title?: string) =>
    call<Meeting>("import_media", { path, title: title || null }),

  // models
  listModels: () => call<ModelEntry[]>("list_models"),
  downloadModel: (id: string) => call<void>("download_model", { id }),
  deleteModel: (id: string) => call<void>("delete_model", { id }),

  // diarization
  diarizationStatus: () => call<boolean>("diarization_status"),
  enableDiarization: () => call<void>("enable_diarization"),

  // audio
  listAudioDevices: () => call<string[]>("list_audio_devices"),

  // settings & templates
  getSettings: () => call<AppSettings>("get_settings"),
  setSettings: (settings: AppSettings) => call<void>("set_settings", { settings }),
  listTemplates: () => call<SummaryTemplate[]>("list_templates"),
  listOllamaModels: (baseUrl: string) =>
    call<string[]>("list_ollama_models", { baseUrl }),

  // summaries
  generateSummary: (meetingId: string, templateId?: string, language?: string) =>
    call<Summary>("generate_summary", {
      meetingId,
      templateId: templateId || null,
      language: language || null,
    }),

  // export
  exportMarkdown: (meetingId: string) =>
    call<string>("export_markdown", { meetingId }),
  saveMarkdown: (meetingId: string, path: string) =>
    call<void>("save_markdown", { meetingId, path }),
};

export const events = {
  onTranscriptSegment: (handler: (e: SegmentEvent) => void): Promise<UnlistenFn> =>
    listen<SegmentEvent>("transcript-segment", (e) => handler(e.payload)),
  onModelDownloadProgress: (
    handler: (e: DownloadProgressEvent) => void,
  ): Promise<UnlistenFn> =>
    listen<DownloadProgressEvent>("model-download-progress", (e) =>
      handler(e.payload),
    ),
};
