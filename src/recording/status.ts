import type { HistoryEntry, RecordingStatus } from "../lib/tauri";

/** Rejects snapshots/events that were captured before the latest UI update. */
export function shouldApplyRecordingStatus(
  latestRevision: number,
  candidate: RecordingStatus,
): boolean {
  return latestRevision < 0 || candidate.revision > latestRevision;
}

export function historyEntrySessionId(entry: HistoryEntry): string | null {
  for (const run of entry.pipeline_runs ?? []) {
    if (run.session_id) return run.session_id;
  }
  return null;
}

export function belongsToRecordingSession(
  entry: HistoryEntry,
  sessionId: string | null,
): boolean {
  const entrySessionId = historyEntrySessionId(entry);
  return !sessionId || entrySessionId === sessionId;
}
