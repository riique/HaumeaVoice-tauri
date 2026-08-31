import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { normalizeAudioBytes } from "../../src/lib/audio-bytes.ts";

test("normalizeAudioBytes preserves raw IPC payloads", () => {
  const buffer = new Uint8Array([0x52, 0x49, 0x46, 0x46]).buffer;
  assert.deepEqual([...normalizeAudioBytes(buffer)], [0x52, 0x49, 0x46, 0x46]);
  assert.deepEqual([...normalizeAudioBytes([0, 127, 255])], [0, 127, 255]);
});

test("normalizeAudioBytes rejects malformed payloads", () => {
  assert.throws(() => normalizeAudioBytes([0, 256]), /inválida/);
});

test("Tauri CSP allows Blob URLs used by History audio", async () => {
  const config = JSON.parse(await readFile(new URL("../../src-tauri/tauri.conf.json", import.meta.url), "utf8"));
  assert.match(config.app.security.csp, /media-src[^;]*blob:/);
});
