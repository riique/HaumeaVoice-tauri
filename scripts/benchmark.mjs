import { readFile, writeFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";

export function distance(reference, hypothesis) {
  let previous = Array.from({ length: hypothesis.length + 1 }, (_, index) => index);
  for (let i = 0; i < reference.length; i++) {
    const current = [i + 1];
    for (let j = 0; j < hypothesis.length; j++) current[j + 1] = Math.min(
      current[j] + 1, previous[j + 1] + 1, previous[j] + Number(reference[i] !== hypothesis[j]),
    );
    previous = current;
  }
  return previous[hypothesis.length];
}
const normalize = (text) => text.normalize("NFC").toLocaleLowerCase("pt-BR").replace(/[^\p{L}\p{N}\s]/gu, " ").trim().replace(/\s+/g, " ");
const words = (text) => text ? text.split(" ") : [];
const percentile = (values, fraction) => values.length ? [...values].sort((a, b) => a - b)[Math.max(0, Math.ceil(values.length * fraction) - 1)] : null;
export function evaluate(records) {
  let wordErrors = 0, wordCount = 0, charErrors = 0, charCount = 0, terms = 0, matches = 0;
  const latency = [], cost = [], duration = [];
  for (const record of records) {
    if (typeof record.reference !== "string" || typeof record.hypothesis !== "string" || record.reference.length > 10_000 || record.hypothesis.length > 10_000) throw new Error("Each segment needs reference/hypothesis strings of at most 10,000 characters");
    const reference = normalize(record.reference), hypothesis = normalize(record.hypothesis);
    const referenceWords = words(reference);
    wordErrors += distance(referenceWords, words(hypothesis)); wordCount += referenceWords.length;
    charErrors += distance([...reference], [...hypothesis]); charCount += [...reference].length;
    for (const term of record.terms ?? []) {
      if (typeof term !== "string" || !term || !record.reference.includes(term)) throw new Error("Terms must occur literally in the reference");
      terms++; matches += Number(record.hypothesis.includes(term));
    }
    for (const [key, target] of [["latency_ms", latency], ["cost_usd", cost], ["audio_seconds", duration]]) {
      if (record[key] == null) continue;
      if (typeof record[key] !== "number" || !Number.isFinite(record[key]) || record[key] < 0) throw new Error(`Invalid ${key}`);
      target.push(record[key]);
    }
  }
  const seconds = duration.reduce((a, b) => a + b, 0);
  return { segments: records.length, word_errors: wordErrors, reference_words: wordCount, wer: wordCount ? wordErrors / wordCount : null,
    character_errors: charErrors, reference_characters: charCount, cer: charCount ? charErrors / charCount : null,
    literal_terms_preserved: matches, literal_terms_total: terms, literal_term_rate: terms ? matches / terms : null,
    latency_samples: latency.length, latency_p50_ms: percentile(latency, .5), latency_p95_ms: percentile(latency, .95),
    cost_usd_per_audio_minute: cost.length === records.length && duration.length === records.length && seconds > 0 ? cost.reduce((a, b) => a + b, 0) / (seconds / 60) : null };
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [source, destination] = process.argv.slice(2);
  if (!source || !destination) throw new Error("Usage: npm run benchmark -- segments.jsonl results.json");
  const records = (await readFile(source, "utf8")).split(/\r?\n/).filter((line) => line.trim()).map((line) => JSON.parse(line));
  const grouped = Object.groupBy(records, (record) => record.pipeline ?? "unspecified");
  const results = { measured_at: new Date().toISOString(), source, network_requests: 0,
    normalization: "WER/CER: NFC, lowercase, punctuation removed; terms: exact literal match", pipelines: Object.fromEntries(Object.entries(grouped).map(([name, entries]) => [name, evaluate(entries)])) };
  await writeFile(destination, `${JSON.stringify(results, null, 2)}\n`, { flag: "wx" });
  console.log(`Evaluated ${records.length} supplied segments locally; no provider was called.`);
}
