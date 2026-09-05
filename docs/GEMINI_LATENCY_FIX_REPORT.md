# Correção de latência Gemini — relatório

**Data:** 2026-07-18
**Produto:** Sonora 1.0.3

---

## Arquivos alterados

| Área | Arquivos |
|------|----------|
| Transporte | `src-tauri/src/gemini/transport.rs` (novo) |
| Cliente / Files | `gemini/client.rs`, `gemini/files.rs`, `gemini/types.rs`, `gemini/mod.rs` |
| STT / refine | `gemini/transcription.rs`, `gemini/refinement.rs`, `gemini/prompts.rs`, `gemini/pronunciation.rs`, `gemini/mock.rs` |
| Modos | `transcription/modes.rs`, `transcription/pipeline.rs`, `transcription/mod.rs` |
| Histórico | `models.rs`, `pipeline_contract.rs` |
| Vocabulário | `vocabulary.rs`, `settings.rs` |
| Áudio | `audio.rs` |
| UI | `src/views/HistoricoView.tsx`, `src/lib/tauri.ts` |
| Docs | este relatório |

---

## Arquitetura anterior → nova

### Antes
```text
todo áudio Gemini → Files API (start + upload + poll 800ms + generate + await delete)
UltraPrecise: join(Whisper, upload) → sanitizer → Gemini  (sanitizer espera upload)
histórico: sanitizer_latency_ms = 0 sempre nos modos
UI: “Xs motor + 0,00s sanitizador”
Rápido e preciso: sem glossário
```

### Depois
```text
áudio curto (≤10 MB e ≤5 min):
  Base64 1× → generateContent inline_data → strict literals
  (sem upload / poll / delete)

áudio grande:
  Files API → poll ACTIVE → generate → texto
  cleanup assíncrono (fora da critical path)

UltraPrecise:
  (Whisper → sanitizer) ∥ (Base64 | upload+poll) → Gemini → strict

telemetria estruturada por etapa + transporte no histórico
```

---

## Inline versus Files API

| Critério | Inline | Files API |
|----------|--------|-----------|
| Tamanho | ≤ 10 MB | > 10 MB |
| Duração | ≤ 5 min | > 5 min |
| Ambos limites | **os dois** devem passar para Inline | qualquer excesso |
| Duração desconhecida | tratada como 0 ms (gate = bytes) | se bytes > 10 MB |
| Round trips Google (feliz) | **1** (`generateContent`) | **4+** (start, bytes, poll×N, generate) + delete async |

Função: `select_gemini_audio_transport` em `gemini/transport.rs` (testada).

---

## Paralelismo

| Modo | Paralelismo |
|------|-------------|
| Rápido e preciso | sequencial (só Gemini) |
| Preciso | Whisper ∥ (Base64 **ou** upload) → refine |
| Ultrapreciso | (Whisper → sanitizer) ∥ (Base64 **ou** upload) → refine |

Sanitizer **não** espera mais o upload ficar ACTIVE.

---

## Cleanup remoto

```text
generate → devolver texto ao usuário
         └→ spawn_cleanup(guard)  // loga delete_ms; Drop como fallback
```

Transcrição **não** falha se delete falhar.

---

## Telemetria

Campos novos em `HistoryEntry` (todos `#[serde(default)]`, retrocompatíveis):

`audio_prepare_ms`, `base64_ms`, `whisper_ms`, `sanitizer_ms`, `files_upload_ms`, `files_poll_ms`, `files_poll_count`, `gemini_generate_ms`, `gemini_delete_ms`, `strict_literals_ms`, `clipboard_ms`, `total_pipeline_ms`, `gemini_transport`

- `sanitizer_latency_ms` nos modos = **tempo real do sanitizer** ou `None` (não mais `Some(0)` forçado).
- UI: resumo “Total · transporte · Whisper · Gemini”; tabela de etapas em Detalhes.
- Histórico antigo: “Detalhamento indisponível para esta transcrição.”

---

## Literais

- Prompt FastAccurate inclui glossário + regras de identificadores.
- `apply_strict_literals` em todos os modos de produto (e pós-Gemini).
- Default seed: **Sonora** `[LITERAL]` com aliases Homey / HowMeia / Homeia / Raumea Voice (`ensure_default_product_terms` no load de settings).

---

## Testes

```text
cargo test  → 85 passed
npm run build → ok
```

Cobertura relevante: transport selection, Sonora aliases, history fields (sanitizer_ms, transport), mocks Files, prompts.

---

## Compatibilidade

Preservado: Ultrarrápido, Deepgram/legado, pronúncia (inline separado), clipboard, gadget, atalhos, retry, upload de arquivo, settings, históricos antigos (campos default).

---

## Riscos restantes

| Risco | Notas |
|-------|--------|
| Limite inline da API Google | 10 MB é conservador; se a API rejeitar, fallback Files no futuro |
| Delete assíncrono | pode não completar se o processo for morto na hora; Drop/best-effort |
| `generationConfig.temperature=0` | suportado no generateContent; thinking não configurado (sem campo inventado) |
| Latência real ainda depende da rede/modelo | inline remove 3–5 s de Files overhead, não o generate em si |

---

## Round trips eliminados (áudio curto)

| Antes | Depois |
|-------|--------|
| start upload | — |
| upload bytes | — |
| poll × N (800 ms) | — |
| generate | generate |
| await delete | async background |

**Economia típica em mic curto:** ~2–6 s de overhead Files + delete síncrono.

---

## Tabela de problemas

| Problema | Correção | Evidência |
|----------|----------|-----------|
| 7–10 s em áudio curto | Inline Base64 para clips ≤10 MB / ≤5 min | `transport.rs`, `transcription.rs` |
| Files API em todo áudio | Seleção híbrida | `select_gemini_audio_transport` |
| Delete síncrono na critical path | `spawn_cleanup` | `files.rs`, refine/transcribe |
| Sanitizer espera upload (Ultra) | Chain Whisper→S ∥ prep | `run_ultra_precise` |
| `sanitizer_latency_ms = 0` | grava `sanitizer_ms` real / `None` | `mode_result_to_history` |
| UI “motor + sanitizador” | resumo Total + etapas | `HistoricoView.tsx` |
| FastAccurate sem glossário | prompt + glossary + strict | `fast_accurate_transcription_prompt` |
| Homey Voice | default literal + aliases | `default_sonora_term` |
| Métricas agregadas | campos por etapa + transport | `HistoryEntry` |

---

CORREÇÕES DO GEMINI CONCLUÍDAS — PRONTO PARA TESTE REAL DE LATÊNCIA
