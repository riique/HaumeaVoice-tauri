import { test } from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const source = await readFile(new URL("../../browser-extension/content.js", import.meta.url), "utf8");
function fixture({ focused = true, visible = true } = {}) {
  let receiver;
  const reads = { selection: 0, title: 0, nearby: 0 };
  class Element { getAttribute() { return null; } }
  const document = {
    hasFocus: () => focused, visibilityState: visible ? "visible" : "hidden",
    activeElement: new Element(),
    get title() { reads.title++; return "Synthetic page"; },
  };
  const window = { getSelection: () => {
    reads.selection++;
    return { toString: () => "Synthetic selection", rangeCount: 1, getRangeAt: () => {
      reads.nearby++;
      return { startContainer: { textContent: "Synthetic paragraph", nodeType: 3 }, startOffset: 0 };
    } };
  } };
  const context = vm.createContext({ document, window, location: { protocol: "https:", hostname: "example.test" },
    HTMLElement: Element, HTMLInputElement: class extends Element {}, HTMLTextAreaElement: class extends Element {},
    Node: { TEXT_NODE: 3 }, chrome: { runtime: { id: "test-extension", onMessage: { addListener: (fn) => { receiver = fn; } } } } });
  vm.runInContext(source, context);
  return { reads, send(request, sender = "test-extension") {
    let response;
    receiver({ type: "sonora-capture", request }, { id: sender }, (value) => { response = value; });
    return response;
  } };
}
const request = (overrides = {}) => ({ request_id: "nonce", expires_at_ms: Date.now() + 1000, title: false, selection: false, nearby_text: false, ...overrides });
test("extension reads no page text before a request or for disabled sources", () => {
  const page = fixture();
  assert.deepEqual(page.reads, { selection: 0, title: 0, nearby: 0 });
  const result = page.send(request());
  assert.equal(result.context.selection, null);
  assert.equal(result.context.title, null);
  assert.deepEqual(page.reads, { selection: 0, title: 0, nearby: 0 });
});
test("extension rejects expired, background and foreign requests before reading", () => {
  for (const options of [{ focused: false }, { visible: false }, {}]) {
    const page = fixture(options);
    assert.equal(page.send(request({ expires_at_ms: 0, title: true, selection: true })), undefined);
    if (Object.keys(options).length) assert.equal(page.send(request({ title: true })), undefined);
    assert.equal(page.send(request({ title: true }), "another-extension"), undefined);
    assert.deepEqual(page.reads, { selection: 0, title: 0, nearby: 0 });
  }
});
test("extension collects only the requested source and returns the nonce", () => {
  const page = fixture();
  const result = page.send(request({ selection: true }));
  assert.equal(result.request_id, "nonce");
  assert.equal(result.context.selection, "Synthetic selection");
  assert.deepEqual(page.reads, { selection: 1, title: 0, nearby: 0 });
});
