import assert from "node:assert/strict";
import test from "node:test";
import {
  belongsToRecordingSession,
  historyEntrySessionId,
  shouldApplyRecordingStatus,
} from "../../src/recording/status.ts";

function status(revision) {
  return {
    generation: 3,
    revision,
    session_id: "recording-session-3",
    phase: "recording",
    recording: true,
    busy: true,
  };
}

test("recording snapshots cannot overwrite a newer lifecycle event", () => {
  assert.equal(shouldApplyRecordingStatus(-1, status(0)), true);
  assert.equal(shouldApplyRecordingStatus(4, status(3)), false);
  assert.equal(shouldApplyRecordingStatus(4, status(4)), false);
  assert.equal(shouldApplyRecordingStatus(4, status(5)), true);
});

test("pipeline completion only controls its matching recording session", () => {
  const entry = {
    pipeline_runs: [{ session_id: "recording-session-2" }],
  };
  assert.equal(historyEntrySessionId(entry), "recording-session-2");
  assert.equal(belongsToRecordingSession(entry, "recording-session-2"), true);
  assert.equal(belongsToRecordingSession(entry, "recording-session-3"), false);
  assert.equal(belongsToRecordingSession(entry, null), true);
  assert.equal(belongsToRecordingSession({}, "recording-session-3"), false);
});
