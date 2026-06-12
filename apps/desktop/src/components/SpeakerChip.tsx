/** Compact colored label for a diarized speaker. */

const PALETTE = [
  "text-tide-300 bg-tide-500/15",
  "text-amber-glow bg-amber-glow/15",
  "text-sky-300 bg-sky-500/15",
  "text-rose-300 bg-rose-500/15",
  "text-violet-300 bg-violet-500/15",
  "text-lime-300 bg-lime-500/15",
  "text-orange-300 bg-orange-500/15",
  "text-cyan-300 bg-cyan-500/15",
];

export function speakerColor(speaker: number): string {
  return PALETTE[(speaker - 1) % PALETTE.length];
}

export function SpeakerChip({ speaker }: { speaker: number }) {
  return (
    <span
      className={`inline-block rounded px-1.5 py-px font-mono text-[9.5px] font-medium uppercase tracking-wider ${speakerColor(speaker)}`}
    >
      Speaker {speaker}
    </span>
  );
}
