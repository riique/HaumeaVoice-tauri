import { test } from "node:test";
import assert from "node:assert/strict";
import { evaluate } from "../../scripts/benchmark.mjs";
test("benchmark measures substitutions, insertions and literal code preservation", () => {
  const result = evaluate([{ reference: "Use o useEffect agora", hypothesis: "Use o use effect agora sim", terms: ["useEffect"] }]);
  assert.equal(result.reference_words, 4); assert.equal(result.word_errors, 3);
  assert.equal(result.literal_terms_preserved, 0); assert.equal(result.wer, .75);
  assert.equal(result.cost_usd_per_audio_minute, null);
});
test("benchmark combines measured latency and cost without inventing missing values", () => {
  const records = [10, 20, 30, 40].map((latency_ms) => ({ reference: "olá", hypothesis: "Olá!", latency_ms, audio_seconds: 15, cost_usd: .01 }));
  const result = evaluate(records);
  assert.equal(result.wer, 0); assert.equal(result.cer, 0);
  assert.equal(result.latency_p50_ms, 20); assert.equal(result.latency_p95_ms, 40);
  assert.equal(result.cost_usd_per_audio_minute, .04);
  records[0].cost_usd = null; assert.equal(evaluate(records).cost_usd_per_audio_minute, null);
});
