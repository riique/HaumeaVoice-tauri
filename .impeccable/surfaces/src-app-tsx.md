---
version: 1
slug: "src-app-tsx"
primary_target: "src/App.tsx"
related_targets: ["src/views/GadgetView.tsx"]
---

## Scope and mode

Replacement visual world for the complete Sonora desktop frontend and its floating gadget. Mode: Operate.

## Audience and job

Windows users dictate into other applications throughout the day, then open Sonora only to manage history, files, shortcuts, pipelines, providers, vocabulary and diagnostics.

## Primary tasks and evidence

The main shell must make current pipeline state, recent local activity and configuration legible without becoming a dashboard. The gadget must communicate capture and recovery using real backend events and RMS audio levels. Code, local persisted data and Tauri IPC are the only authority for capabilities.

## Constraints

Preserve all existing workflows, pt-BR copy strategy, keyboard access, click-through outside the gadget, always-on-top behavior, multi-monitor/DPI position restore and local-only credential handling. Do not add cloud sync, help, changelog or claims inferred from mockups.

## Chosen direction

Quiet Windows utility: warm-white canvas, neutral sidebar, white working surfaces, near-black controls and one workhorse system sans. Hierarchy comes from spacing, type, rules and restrained elevation. Dense technical power is progressively disclosed.

## Memorable moment

The black Sonora Bar rests as a compact lozenge, expands only when state demands it and turns real speech energy into a calm white waveform.

## Unresolved decisions

None. Direction and code-first workflow are explicitly pinned by the user.
