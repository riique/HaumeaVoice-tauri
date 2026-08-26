import assert from "node:assert/strict";
import test from "node:test";
import { showsProcessingLabel, stateAfterTimeout } from "../../src/gadget/machine.ts";

test("compact processing never renders a label inside the dots-only pill", () => {
  assert.equal(showsProcessingLabel("processing"), false);
  assert.equal(showsProcessingLabel("processing_long"), true);
});

test("compact processing promotes to the wide state after its timeout", () => {
  assert.equal(stateAfterTimeout("processing", "auto"), "processing_long");
});
