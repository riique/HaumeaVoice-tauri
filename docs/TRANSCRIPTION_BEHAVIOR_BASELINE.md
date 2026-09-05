# Baseline de comportamento — Transcrição (Fase 00)

Documento de referência do comportamento **atual** do Sonora **antes** da migração para os novos modos.
Nenhuma alteração funcional foi feita nesta fase. Validado contra `APP_CONTEXT_SONORA.md` e builds locais.

**Versão do app:** 1.0.3
**Data do baseline:** 2026-07-18
**Identificador:** `com.haumeavoice.app`

---

## 1. Resultado dos comandos de baseline

| Comando | Diretório | Resultado | Notas |
|---------|-----------|-----------|--------|
| `npm run build` | raiz | **OK** | `tsc && vite build` — 1758 modules; `dist/` gerado |
| `cargo check` | `src-tauri/` | **OK** | Sem warnings capturados |
| `cargo test` | `src-tauri/` | **OK** | 3 testes passaram; 0 falhas |

### Testes Rust executados

```text
gadget_placement_tests::keeps_a_valid_secondary_monitor_position_and_dpi ... ok
gadget_placement_tests::recovers_a_position_from_a_disconnected_monitor ... ok
gadget_placement_tests::clamps_negative_edges_without_forcing_the_primary_monitor ... ok
```

### O que **não** foi executado (propositalmente)

- Chamadas reais a Groq / Deepgram / Gemini
- `npm run tauri build` (empacotamento completo; não exigido na Fase 00)
- Testes E2E de gravação/microfone

---

## 2. Validação do `APP_CONTEXT_SONORA.md`

| Afirmação do relatório | Status |
|------------------------|--------|
| Stack Tauri 2 + React/TS + Rust | Confirmada (`package.json`, `Cargo.toml`) |
| Orquestração em `audio.rs` | Confirmada |
| Motores STT: Groq Whisper + Deepgram | Confirmada |
| Dual engine paralelo | Confirmada |
| Sanitizer Groq multi-modelo | Confirmada |
| Gemini só avaliação de pronúncia | Confirmada (`gemini.rs`; STT rejeita `GeminiMultimodal`) |
| Deepgram permanece no código | Confirmada — **não remover** |
| Histórico JSON + áudio local | Confirmada |
| Clipboard + Ctrl+V (fonte mic) | Confirmada |
| Gadget + atalhos + tray | Confirmada |
| OpenRouter ausente | Confirmada |
| Testes de pipeline quase nulos | Confirmada (só 3 testes de placement) |

Nenhuma divergência material encontrada entre o relatório e o código nesta fase.

---

## 3. Fluxo funcional que deve ser preservado

### 3.1 Gravação por microfone

1. Atalho global (default `Control+B`) ou botão em Início → `toggle_recording_state` / `handle_toggle`.
2. Unmute de endpoints de captura (Windows COM) → `cpal` captura na taxa nativa.
3. Buffer mono `i16` em RAM; opcional sessão Deepgram live se `streaming_final` + Deepgram no path.
4. Stop → resample 16 kHz mono WAV em RAM → salva em `{app_data}/audio/{id}.wav`.
5. Stage 1 STT conforme configuração (ver §4).
6. Stage 2 sanitizer Groq se `sanitizer_enabled` (default on) e chave Groq presente.
7. Fallbacks: raw acústico se sanitizer falha/vazio/sentinel; preferência Deepgram em empty final.
8. Clipboard + simulação Ctrl+V (somente fonte `mic`).
9. `history.json` + evento `transcription-saved`.
10. Gadget: `recording-*` + `transcribing` + `audio-level`.

### 3.2 Cancelamento

- Atalho cancel (default `Control+Q`) → descarta buffer, aborta Deepgram live, **sem** STT, evento `recording-cancelled`.

### 3.3 Upload de arquivo

- View Transcrição → `transcribe_file` → mesmo Stage 1/2.
- **Não** copia para clipboard.
- `source: "file"`, `duration_ms: 0`.
- Limite 50 MB.

### 3.4 Avaliação de pronúncia

- Histórico → “Avaliar Pronúncia” → Gemini multimodal no áudio salvo + transcript.
- Independente do pipeline de STT.
- **Não alterar** nesta migração de modos.

### 3.5 Superfícies a preservar (não regressar)

| Superfície | Arquivos-chave |
|------------|----------------|
| Atalhos globais | `shortcuts.rs`, `AtalhosView.tsx` |
| Gadget overlay | `lib.rs`, `GadgetView.tsx` |
| Clipboard / paste | `audio.rs` (`paste_into_focused_field`) |
| Histórico | `history.rs`, `HistoricoView.tsx` |
| Avaliação de pronúncia | `gemini.rs`, `PronunciationEvaluation.tsx` |
| Tray / close-to-tray | `lib.rs` |
| Vocabulário no sanitizer | `custom_words` + `call_sanitizer_api` |

---

## 4. Matriz de configuração atual (pré-modos)

| Preferência | Valores | Default | Persistência |
|-------------|---------|---------|--------------|
| `engine` | `groq-whisper`, `deepgram-nova3`, (`gemini-multimodal` enum only) | `groq-whisper` | `settings.json` |
| `dual_engine` | bool | `false` | `settings.json` |
| `deepgram_mode` | `batch`, `streaming_final` | `batch` | `settings.json` |
| `sanitizer` | llama-70b, gpt-oss-20b, gpt-oss-120b, qwen3-27b | llama-70b | `settings.json` |
| `sanitizer_enabled` | bool | `true` | `settings.json` |
| `reasoning_enabled` / `effort` | bool + low/medium/high | false / medium | `settings.json` |

### Combinações efetivas de STT hoje

| dual | engine | Comportamento real |
|------|--------|-------------------|
| false | GroqWhisper | Só Whisper |
| false | DeepgramNova3 | Só Deepgram (batch ou live/streaming) |
| false | GeminiMultimodal | **Erro** se selecionado no pipeline (“não conectado”) |
| true | (engine ignorado para STT) | Whisper ∥ Deepgram; sanitizer mescla se on |

---

## 5. Contratos de API (sem chaves)

### Groq Whisper

- `POST https://api.groq.com/openai/v1/audio/transcriptions`
- Modelo: `whisper-large-v3-turbo`
- Timeout: 30s

### Groq Sanitizer

- `POST https://api.groq.com/openai/v1/chat/completions`
- `temperature: 0.0`
- User: `[WHISPER_RAW]` / `[DEEPGRAM_RAW]`
- Sentinel: `[FALLBACK_RETRY]`

### Deepgram

- Batch: `POST https://api.deepgram.com/v1/listen` (`nova-3`, `language=pt-BR`, `keyterm=Sonora`)
- Live WS: `wss://api.deepgram.com/v1/listen` (`interim_results=false`)

### Gemini (apenas pronúncia)

- `gemini-3.5-flash` via Generative Language API
- Timeout: 120s

---

## 6. Persistência local (não alterar layout sem migração)

```text
%APPDATA%\com.haumeavoice.app\
  history.json
  api_keys.json          # plain text — não logar
  settings.json
  shortcuts.json
  audio\{id}.{ext}
  logs\app.log
  logs\crash.log
```

Limites: 200 entradas de histórico; áudio das 10 mais recentes.

---

## 7. Arquivos frágeis (qualquer mudança de pipeline toca aqui)

| Prioridade | Arquivo | Motivo |
|------------|---------|--------|
| P0 | `src-tauri/src/audio.rs` | Orquestração STT + sanitize + clipboard + history |
| P0 | `src-tauri/src/models.rs` | Enums, `AppState`, `HistoryEntry` |
| P0 | `src-tauri/src/commands.rs` | IPC surface |
| P0 | `src-tauri/src/groq.rs` | Whisper + sanitizer |
| P1 | `src-tauri/src/deepgram.rs` | Manter intacto até decisão de produto |
| P1 | `src-tauri/src/gemini.rs` | Não misturar com STT novo sem separar |
| P1 | `src-tauri/src/settings.rs` | Defaults e persistência |
| P1 | `src/lib/tauri.ts` | Tipos IPC frontend |
| P1 | `src/views/ConfiguracoesView.tsx` | UI de motores |
| P2 | `src/views/HistoricoView.tsx` | Labels/métricas |
| P2 | `src/views/TranscricaoView.tsx` | Upload |
| P2 | `src-tauri/src/lib.rs` | Registro de commands |

---

## 8. Critérios de não-regressão (checklist pós-fase futura)

Usar após cada fase de implementação:

- [ ] Gravação inicia/para por atalho e por botão
- [ ] Cancel descarta sem histórico de sucesso
- [ ] Texto do mic chega ao clipboard e cola no campo focado
- [ ] Entrada aparece no Histórico com áudio (quando aplicável)
- [ ] Dual + Deepgram batch/streaming ainda funcionam se não removidos
- [ ] Sanitizer on/off e modelos Groq inalterados em comportamento
- [ ] Avaliar Pronúncia ainda usa Gemini no áudio do card
- [ ] Gadget: idle / gravando / processando
- [ ] Upload de arquivo não sobrescreve clipboard
- [ ] Retry de falha reprocessa áudio salvo
- [ ] `npm run build` e `cargo check` / `cargo test` verdes

---

## 9. Lacunas conhecidas (pré-existentes — não “corrigir” na Fase 00)

- Select de idioma na Transcrição é cosmético
- Toggle “Minimizar para bandeja” só no React
- System prompt sem editor na UI
- Gemini STT no enum mas fora do pipeline
- Chaves plain text + expostas ao frontend
- Cobertura de testes de pipeline ≈ 0

Estas lacunas são baseline, não bugs introduzidos pela migração.

---

## 10. Git / checkpoint

Ver `docs/TRANSCRIPTION_MIGRATION_PLAN.md` § Checkpoint.
