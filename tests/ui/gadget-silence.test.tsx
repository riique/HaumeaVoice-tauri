import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
const bridge = vi.hoisted(() => ({ invoke: vi.fn(), listeners: new Map<string, (event: { payload: unknown }) => void>() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: bridge.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async (name: string, callback: (event: { payload: unknown }) => void) => { bridge.listeners.set(name, callback); return () => bridge.listeners.delete(name); }) }));
import { GadgetApp } from "../../src/views/GadgetView";
let host: HTMLDivElement;
let root: Root;
const status = (revision: number, phase: string, session_id: string | null = "test-mic") => ({ generation: 1, revision, phase, session_id, recording: phase === "recording", busy: phase !== "idle" });
const emit = async (name: string, payload: unknown) => { await act(async () => bridge.listeners.get(name)?.({ payload })); };
beforeEach(async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers(); bridge.listeners.clear(); bridge.invoke.mockReset();
  window.matchMedia = vi.fn().mockReturnValue({ matches: true });
  bridge.invoke.mockImplementation(async (command: string, args: { visualState?: string } = {}) => {
    if (command === "get_recording_status") return status(3, "stopping");
    if (command === "get_widget_preferences") return { visibility_mode: "auto", dock: "bottom" };
    if (command === "get_shortcuts") return { toggle: "Control+B", cancel: "Control+Q" };
    if (command === "set_gadget_visual_state") return { visual_state: args.visualState, generation: 1 };
    if (command === "acknowledge_gadget_rendered") return true;
    throw new Error(`Unexpected command ${command}`);
  });
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  await act(async () => root.render(<GadgetApp />));
});
afterEach(async () => { await act(async () => root.unmount()); host.remove(); vi.useRealTimers(); });

test("local silence stays neutral through processing completion, then disappears", async () => {
  await emit("transcribing", { active: true, operation_id: 1, cancelled: false });
  await emit("recording-no-speech", status(3, "stopping"));
  await emit("transcribing", { active: false, operation_id: 1, cancelled: false });
  await emit("recording-idle", status(4, "idle", null));
  expect(host.textContent).toContain("Nenhuma voz encontrada");
  expect(host.querySelector('[role="alert"]')).toBeNull();
  expect(host.querySelector('[aria-label*="Tentar transcrever"]')).toBeNull();
  await act(async () => vi.advanceTimersByTime(3300));
  expect(host.textContent).not.toContain("Nenhuma voz encontrada");
});

test("stale silence cannot replace a new recording or disguise a real error", async () => {
  await emit("recording-started", status(5, "recording", "new-session"));
  await emit("recording-no-speech", status(3, "stopping"));
  expect(host.querySelector('[aria-label="Parar e transcrever"]')).not.toBeNull();
  await emit("capture-error", "Microfone indisponível");
  expect(host.querySelector('[role="alert"]')?.textContent).toContain("Microfone indisponível");
  expect(host.textContent).not.toContain("Nenhuma voz encontrada");
});
