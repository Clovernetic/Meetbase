/** Global UI state: navigation and the live recording session. */

import { create } from "zustand";
import { api, events } from "./api";
import type { Segment } from "./types";

export type View =
  | { name: "record" }
  | { name: "library" }
  | { name: "meeting"; id: string }
  | { name: "settings" };

interface RecordingState {
  meetingId: string | null;
  /** Wall-clock start, for the timer. */
  startedAt: number | null;
  liveSegments: Segment[];
  stopping: boolean;
}

interface AppStore {
  view: View;
  navigate: (view: View) => void;

  recording: RecordingState;
  startRecording: (title?: string) => Promise<void>;
  stopRecording: () => Promise<string | null>;
  /** Re-attaches to a recording after a frontend reload. */
  syncRecording: () => Promise<void>;
}

const idleRecording: RecordingState = {
  meetingId: null,
  startedAt: null,
  liveSegments: [],
  stopping: false,
};

export const useStore = create<AppStore>((set, get) => {
  // One global listener keeps live segments flowing regardless of view.
  void events.onTranscriptSegment(({ meetingId, segment }) => {
    const { recording } = get();
    if (recording.meetingId === meetingId) {
      set({
        recording: {
          ...recording,
          liveSegments: [...recording.liveSegments, segment],
        },
      });
    }
  });

  return {
    view: { name: "record" },
    navigate: (view) => set({ view }),

    recording: idleRecording,

    startRecording: async (title) => {
      const meeting = await api.startRecording(title);
      set({
        recording: {
          meetingId: meeting.id,
          startedAt: Date.now(),
          liveSegments: [],
          stopping: false,
        },
      });
    },

    stopRecording: async () => {
      const { recording } = get();
      if (!recording.meetingId) return null;
      set({ recording: { ...recording, stopping: true } });
      try {
        const result = await api.stopRecording();
        set({ recording: idleRecording });
        return result.meetingId;
      } catch (e) {
        set({ recording: { ...recording, stopping: false } });
        throw e;
      }
    },

    syncRecording: async () => {
      const status = await api.recordingStatus();
      if (status.meetingId) {
        const detail = await api.getMeeting(status.meetingId);
        set({
          recording: {
            meetingId: status.meetingId,
            startedAt: Date.now() - status.elapsedMs,
            liveSegments: detail.segments,
            stopping: false,
          },
        });
      }
    },
  };
});
