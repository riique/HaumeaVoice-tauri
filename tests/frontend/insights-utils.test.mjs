import assert from "node:assert/strict";
import test from "node:test";
import {
  adjacentInsightsTab,
  buildActivityCells,
  formatInsightNumber,
  voiceProfileProgress,
} from "../../src/views/insights-utils.ts";

test("activity projection covers 91 days and preserves real counts", () => {
  const now = new Date(2026, 7, 26, 12, 0, 0);
  const cells = buildActivityCells([
    { day: "2026-08-25", sessions: 3 },
    { day: "2026-08-26", sessions: 2 },
  ], now);

  assert.equal(cells.length, 91);
  assert.deepEqual(cells.at(-2), { key: "2026-08-25", count: 3 });
  assert.deepEqual(cells.at(-1), { key: "2026-08-26", count: 2 });
  assert.equal(cells.filter((cell) => cell.count > 0).length, 2);
});

test("tab keyboard navigation follows the WAI-ARIA tab order", () => {
  assert.equal(adjacentInsightsTab("voice", "ArrowLeft"), "usage");
  assert.equal(adjacentInsightsTab("usage", "ArrowRight"), "voice");
  assert.equal(adjacentInsightsTab("voice", "Home"), "usage");
  assert.equal(adjacentInsightsTab("usage", "End"), "voice");
  assert.equal(adjacentInsightsTab("usage", "Enter"), "usage");
});

test("voice profile progress is bounded and handles an empty threshold", () => {
  assert.equal(voiceProfileProgress(2_500, 5_000), 50);
  assert.equal(voiceProfileProgress(7_500, 5_000), 100);
  assert.equal(voiceProfileProgress(-10, 5_000), 0);
  assert.equal(voiceProfileProgress(0, 0), 100);
});

test("numbers use the product pt-BR formatter", () => {
  assert.equal(formatInsightNumber(1234), "1.234");
  assert.equal(formatInsightNumber(4.25, 1), "4,3");
});
