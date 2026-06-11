import { useEffect } from "react";
import { Sidebar } from "./components/Sidebar";
import { RecordView } from "./views/RecordView";
import { LibraryView } from "./views/LibraryView";
import { MeetingView } from "./views/MeetingView";
import { SettingsView } from "./views/SettingsView";
import { useStore } from "./lib/store";

export default function App() {
  const view = useStore((s) => s.view);
  const syncRecording = useStore((s) => s.syncRecording);

  // Re-attach to an in-flight recording if the webview reloaded.
  useEffect(() => {
    void syncRecording().catch(() => {});
  }, [syncRecording]);

  return (
    <div className="flex h-full bg-ink-900 surface-grain">
      <Sidebar />
      <main className="relative z-10 min-w-0 flex-1 overflow-hidden">
        {view.name === "record" && <RecordView />}
        {view.name === "library" && <LibraryView />}
        {view.name === "meeting" && <MeetingView key={view.id} meetingId={view.id} />}
        {view.name === "settings" && <SettingsView />}
      </main>
    </div>
  );
}
