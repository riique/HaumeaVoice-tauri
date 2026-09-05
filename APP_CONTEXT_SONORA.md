# APP_CONTEXT_SONORA.md

> Referência histórica da arquitetura 1.x, renomeada com o produto. Para o estado atual do Sonora v2.0, use [README](README.md), [mudanças da versão 2.0](docs/SONORA_2.0.md) e o código. Descrições antigas de armazenamento, chaves em texto e captura anteriores à auditoria foram superadas pela versão 1.0.34.

Relatório técnico do estado atual do **Sonora** (v1.0.3), gerado por inspeção estática do repositório.
Nenhuma alteração de código, instalação de dependências ou chamada de API foi feita.

**Data da análise:** 2026-07-18  
**Identificador do app:** `com.haumeavoice.app`

---

# 1. Visão geral

## Para que serve

Aplicativo desktop de **digitação por voz**: o usuário grava áudio (atalho global ou botão), o app transcreve via motores em nuvem, opcionalmente **sanitiza** o texto com um LLM, copia para a área de transferência e simula **Ctrl+V** no campo focado. Também permite upload de arquivos de áudio, histórico local, vocabulário personalizado e avaliação de pronúncia (Gemini multimodal).

## Sistema operacional alvo

- **Primário / implementado em profundidade:** Windows (WASAPI/cpal, COM unmute de microfone, tray, registry autostart, gadget HWND/click-through, crash handlers Win32).
- Código condicional `#[cfg(not(target_os = "windows"))]` existe (atalhos/gadget fallbacks), mas o produto é claramente **Windows-first**.

## Stack

| Camada | Tecnologia |
|--------|------------|
| Shell desktop | **Tauri 2** |
| Frontend | **React 18** + **TypeScript** + **Vite 5** + **Tailwind CSS 3** |
| Backend nativo | **Rust 2021** (crate `sonora_lib`) |
| Áudio | **cpal** 0.15 |
| HTTP | **reqwest** 0.12 (multipart + JSON) |
| WebSocket | **tokio-tungstenite** 0.26 (Deepgram live) |
| Clipboard / paste | **arboard** + **enigo** |
| Ícones UI | **lucide-react** |
| Markdown avaliação | **react-markdown** |

## Linguagens

- TypeScript/TSX (UI)
- Rust (captura, pipeline STT, IPC, persistência, OS integration)

## Framework de interface

React SPA embutida no WebView Tauri. Janela principal **sem decorações** (title bar custom). Segunda janela **"gadget"** transparente, always-on-top, frameless.

## Backend

Não há servidor próprio. O “backend” é o processo Rust Tauri que:

1. Captura áudio
2. Chama APIs externas (Groq, Deepgram, Google Gemini)
3. Persiste JSON/arquivos em `%APPDATA%\com.haumeavoice.app\`
4. Expõe comandos IPC (`invoke`) e eventos Tauri

## Empacotamento

- `tauri.conf.json` → bundle `targets: "all"`, productName `"Sonora"`
- Build: `npm run tauri build` (ou `npm run build` + cargo)
- Artefato documentado: `src-tauri/target/release/sonora.exe` (`BUILD.md`)

## Como iniciar

```bash
npm run tauri          # CLI Tauri
# ou
npm run dev            # só Vite (UI sem backend completo)
cargo tauri dev        # via CLI (beforeDevCommand = npm run dev)
```

Autostart Windows: registry `HKCU\...\Run\Sonora` com `"exe" --autostart` (janela main oculta no autostart).

## Como compilar

```bash
npm run build          # tsc && vite build → dist/
npm run tauri build    # frontend + Rust release
```

## Estrutura geral do repositório

```text
Sonora/
├── package.json              # sonora 1.0.3, scripts dev/build/tauri
├── vite.config.ts
├── tailwind.config.js
├── index.html
├── BUILD.md
├── Otimization.txt           # notas de otimização (não código)
├── src/                      # Frontend React
│   ├── main.tsx              # roteia main vs gadget pelo label da janela
│   ├── App.tsx               # shell: TitleBar + Sidebar + views
│   ├── lib/tauri.ts          # wrappers tipados de invoke/listen
│   ├── components/           # UI + PronunciationEvaluation
│   └── views/                # Inicio, Transcricao, Historico, Atalhos, Config, Gadget
└── src-tauri/
    ├── Cargo.toml
    ├── tauri.conf.json
    ├── capabilities/default.json
    └── src/
        ├── main.rs           # entry → sonora_lib::run()
        ├── lib.rs            # setup tray, gadget, logging, IPC handler
        ├── commands.rs       # todos os #[tauri::command]
        ├── models.rs         # estado, enums, HistoryEntry
        ├── audio.rs          # captura, WAV, pipeline STT+sanitize
        ├── groq.rs           # Whisper + sanitizer Chat Completions
        ├── deepgram.rs       # batch REST + streaming WS live
        ├── gemini.rs         # avaliação de pronúncia multimodal
        ├── history.rs / secrets.rs / settings.rs / audio_store.rs
        ├── shortcuts.rs / mic_control.rs
        └── vendor/quote/     # patch local (Kaspersky false positive)
```

**Arquivos de aplicação analisados:** 35 (21 frontend + 14 Rust de `src-tauri/src`, excluindo vendor/target).

---

# 2. Arquitetura atual

## 2.1 Gravação de áudio

```text
Responsabilidade: capturar mic (cpal), buffer mono i16, resample 16 kHz, montar WAV em RAM, opcional live Deepgram
Arquivos:
  - src-tauri/src/audio.rs
  - src-tauri/src/mic_control.rs
  - src-tauri/src/audio_store.rs
Funções/classes principais:
  - start_capture, stop_capture, cancel_capture
  - create_wav_buffer, resample, downmix_i16/f32
  - spawn_audio_level_emitter, start_mic_test_stream
  - ensure_mic_unmuted (Windows COM)
Dependências: cpal, parking_lot, windows (Win32 Media Audio)
Estado atual: IMPLEMENTADO (Windows-first)
```

## 2.2 Captura global de teclado

```text
Responsabilidade: atalhos globais toggle/cancel gravação
Arquivos:
  - src-tauri/src/shortcuts.rs
  - src-tauri/src/commands.rs (get_shortcuts, set_shortcuts)
  - src/views/AtalhosView.tsx
Funções/classes principais:
  - handle_toggle, handle_cancel, register_all, apply_new
  - ShortcutConfig { toggle, cancel } default Control+B / Control+Q
Dependências: tauri-plugin-global-shortcut
Estado atual: IMPLEMENTADO + persistido em shortcuts.json
```

## 2.3 Interface

```text
Responsabilidade: UI desktop + overlay gadget
Arquivos:
  - src/App.tsx, src/main.tsx
  - src/views/*.tsx
  - src/components/*
  - src/lib/tauri.ts
Funções/classes principais:
  - App (navegação local por ViewKey)
  - GadgetApp (overlay)
  - wrappers invoke
Dependências: React, Tauri API, Tailwind, lucide-react
Estado atual: IMPLEMENTADO
  - Estado global de UI: useState por view (sem Redux/Zustand)
  - Estado de negócio: Rust AppState + JSON em disco
```

## 2.4 Transcrição (Stage 1 acústico)

```text
Responsabilidade: STT cloud a partir do WAV/arquivo
Arquivos:
  - src-tauri/src/audio.rs (orquestração)
  - src-tauri/src/groq.rs (Whisper)
  - src-tauri/src/deepgram.rs (Nova-3)
  - src-tauri/src/models.rs (TranscriptionEngine, DeepgramMode)
Funções/classes principais:
  - transcribe_bytes, run_dual_posthoc, deepgram_from_live_or_posthoc
  - call_whisper_api, deepgram::transcribe / spawn_live_session
Dependências: reqwest, tokio-tungstenite
Estado atual:
  - Groq Whisper: IMPLEMENTADO
  - Deepgram batch + streaming_final (live mic): IMPLEMENTADO
  - Dual engine (Whisper ∥ Deepgram): IMPLEMENTADO
  - Gemini como motor de transcrição: NÃO CONECTADO ao pipeline (só UI/enum)
```

## 2.5 Sanitização (Stage 2)

```text
Responsabilidade: LLM limpa/mescla texto bruto antes do clipboard
Arquivos:
  - src-tauri/src/groq.rs (call_sanitizer_api)
  - src-tauri/src/settings.rs (DEFAULT_SYSTEM_PROMPT)
  - src-tauri/src/audio.rs (finalize_transcription)
  - src/views/ConfiguracoesView.tsx (MotoresTab)
Funções/classes principais:
  - call_sanitizer_api, SanitizerOutcome, SanitizerDebug
  - pick_raw_acoustic, coalesce_empty_final
  - SanitizerModel enum
Dependências: Groq Chat Completions + chave Groq
Estado atual: IMPLEMENTADO (pode ser desligado via sanitizer_enabled)
```

## 2.6 Histórico

```text
Responsabilidade: persistir entradas de transcrição e métricas
Arquivos:
  - src-tauri/src/history.rs
  - src-tauri/src/models.rs (HistoryEntry)
  - src/views/HistoricoView.tsx
  - src/views/InicioView.tsx (stats)
Funções/classes principais:
  - history::push, load_all, get, update_entry, set_evaluation, clear
Dependências: serde_json, parking_lot
Estado atual: IMPLEMENTADO (JSON, não SQL)
```

## 2.7 Banco de dados / persistência

```text
Responsabilidade: arquivos JSON + áudios em AppData
Arquivos:
  - history.json, api_keys.json, settings.json, shortcuts.json
  - audio/{id}.{ext}
Local típico Windows: %APPDATA%\com.haumeavoice.app\
Estado atual: IMPLEMENTADO (sem SQLite/Postgres)
```

## 2.8 Configurações

```text
Responsabilidade: preferências UI/engine/sanitizer/mic/dev mode
Arquivos:
  - src-tauri/src/settings.rs
  - src-tauri/src/commands.rs
  - src/views/ConfiguracoesView.tsx
Estado atual: IMPLEMENTADO (parcialmente: toggle “Minimizar para bandeja” é só local no React)
```

## 2.9 Vocabulário

```text
Responsabilidade: lista de grafias canônicas injetada no prompt do sanitizer
Arquivos:
  - settings.custom_words / AppState.custom_words
  - groq::call_sanitizer_api (bloco GLOSSÁRIO PESSOAL)
  - src/views/ConfiguracoesView.tsx (VocabularioTab)
Estado atual: IMPLEMENTADO (lista simples de strings, sem aliases/categorias)
```

## 2.10 Avaliação de pronúncia

```text
Responsabilidade: Gemini multimodal analisa áudio salvo + transcript
Arquivos:
  - src-tauri/src/gemini.rs
  - commands::evaluate_pronunciation
  - src/components/PronunciationEvaluation.tsx
  - src/views/HistoricoView.tsx
Estado atual: IMPLEMENTADO (subjetivo via LLM; UI parseia Markdown estruturado)
```

## 2.11 Provedores externos

| Provedor | Uso real |
|----------|----------|
| Groq | Whisper STT + Chat Completions sanitizer |
| Deepgram | Nova-3 STT (REST e/ou WS) |
| Google Generative Language | Gemini avaliação de pronúncia |
| OpenRouter | **NÃO EXISTE** no código |

## 2.12 Telemetria

```text
Responsabilidade: métricas por entrada de histórico (latência, RTF, tok/s estimados)
Arquivos: audio.rs finalize_transcription, HistoryEntry fields, HistoricoView display
Estado atual: IMPLEMENTADO localmente (sem envio a serviço externo de analytics)
```

## 2.13 Logs

```text
Responsabilidade: env_logger → stderr + %APPDATA%\...\logs\app.log
Crash: panic hook + vectored exception handler → crash.log
Heartbeat: stall AppHangB1 → crash.log
Arquivos: src-tauri/src/lib.rs
Estado atual: IMPLEMENTADO
```

## 2.14 Tratamento de erros

```text
Responsabilidade: CommandError string, history is_error, fallbacks de sanitizer
Arquivos: commands.rs, audio.rs, HistoricoView.tsx
Estado atual: IMPLEMENTADO (ver seção 12)
```

---

# 3. Fluxo completo de gravação

## Passo a passo real

1. **Início**
   - Atalho global (`shortcuts::handle_toggle`) ou botão Início (`toggle_recording_state` → mesmo handler).
   - `recording=true`, evento `recording-started`.
   - Thread background: `mic_control::ensure_mic_unmuted()` → `audio::start_capture`.
   - Se Deepgram `streaming_final` e Deepgram no path: `spawn_live_session` (WS).
   - Emitter de `audio-level` para o gadget (~80 ms).

2. **Onde o áudio fica**
   - Durante captura: **RAM** (`AppState.audio_buffer: Vec<i16>`).
   - Após stop: WAV em RAM + cópia em disco `audio_store::save` → `{app_data}/audio/{id}.wav`.

3. **Formato**
   - Saída: **WAV RIFF PCM**.

4. **Codec**
   - PCM linear 16-bit signed little-endian.

5. **Sample rate**
   - Captura: **taxa nativa do dispositivo** (ex. 48000).
   - Final: **16000 Hz** (`TARGET_SAMPLE_RATE`) via resample linear.

6. **Canais**
   - Saída: **mono** (downmix se multi-canal).

7. **Conversão**
   - F32/I16 nativo → mono i16; resample para 16 kHz; header WAV 44 bytes.

8. **DSP**
   - Apenas downmix + resample linear + RMS para UI. Sem denoise/VAD offline próprio (Deepgram tem `endpointing=300` no WS).

9. **Fim da gravação**
   - Toggle off → `stop_capture` async no runtime Tokio.
   - Drop stream, catch-up Deepgram live, drain buffer, monta WAV.

10. **Envio**
    - Single: Groq multipart **ou** Deepgram (live finish / batch / post-hoc).
    - Dual: `tokio::join!` Whisper + Deepgram.
    - Áudio **não** é “upload remoto permanente”: bytes enviados na request; cópia local permanece.

11. **Resposta**
    - Texto bruto STT → opcional sanitizer Groq → `final_text`.

12. **Exibição**
    - Clipboard + `enigo` Ctrl+V (fonte `mic`).
    - Evento `transcription-saved` → Histórico/Início atualizam.
    - Gadget: evento `transcribing` true/false (“Processando…”).
    - Início **não** mostra o texto final na tela principal (só timer/stats).

13. **Áudio apagado/preservado**
    - Preservado em `audio/` para as **10 entradas mais recentes** com áudio; além disso `audio_path` é removido e arquivo deletado.
    - Histórico de textos até **200** entradas.
    - `clear_history` apaga JSON e todos os áudios.

14. **Erros**
    - STT falha → `HistoryEntry` com `is_error=true`, mensagem PT; sem clipboard.
    - Sanitizer falha → usa `pick_raw_acoustic` (não perde texto).
    - Sanitizer vazio → `coalesce_empty_final` (prefere Deepgram raw).

## Diagrama textual

```text
Usuário (Ctrl+B / botão)
  → shortcuts::handle_toggle
  → mic unmute + cpal start_capture
  → buffer RAM (+ Deepgram live PCM se streaming_final)
  → stop
  → resample 16k mono WAV (RAM)
  → audio_store.save (disco)
  → Stage1 STT:
       [single] Groq Whisper | Deepgram
       [dual]   Groq Whisper ∥ Deepgram
  → Stage2 sanitizer Groq Chat (se enabled + key)
       fallback: pick_raw / sentinel / empty coalesce
  → clipboard + Ctrl+V (mic)
  → history.push + emit transcription-saved
  → UI Histórico / stats Início / gadget idle
```

```text
Gravação
→ processamento (WAV/resample)
→ provedor STT (Groq/Deepgram)
→ sanitização (Groq LLM, opcional)
→ interface (clipboard/paste + eventos)
→ histórico (history.json + audio/)
```

---

# 4. Motores de transcrição atuais

## 4.1 Groq Whisper — IMPLEMENTADO

```text
Nome exibido na interface: Groq Whisper
Modelo real: whisper-large-v3-turbo
Endpoint: https://api.groq.com/openai/v1/audio/transcriptions
Arquivo responsável: src-tauri/src/groq.rs
Parâmetros enviados:
  - multipart: model=whisper-large-v3-turbo, file=(bytes)
  - Authorization: Bearer <GROQ_KEY>
Formato de áudio: WAV (mic) ou container do upload (mp3/m4a/…)
Prompt: nenhum (sem prompt Whisper)
Timeout: 30s (cliente HTTP compartilhado)
Streaming ou batch: batch HTTP
Fallback: no dual, se falha e Deepgram ok → só Deepgram
Retry: não (exceto retry manual do histórico)
Rate limit: não tratado especificamente (HTTP 429 vira ApiError)
Status atual: IMPLEMENTADO (default TranscriptionEngine)
```

Exemplo sanitizado de request:

```http
POST https://api.groq.com/openai/v1/audio/transcriptions
Authorization: Bearer gsk_***
Content-Type: multipart/form-data

model=whisper-large-v3-turbo
file=audio.wav (audio/wav)
```

## 4.2 Deepgram Nova-3 — IMPLEMENTADO

```text
Nome exibido na interface: Deepgram Nova-3
Modelo real: nova-3
Endpoints:
  - Batch: https://api.deepgram.com/v1/listen?...
  - WS:    wss://api.deepgram.com/v1/listen?...
Arquivo responsável: src-tauri/src/deepgram.rs
Parâmetros batch (query):
  model=nova-3
  language=pt-BR
  punctuate=true
  numerals=true
  paragraphs=false
  keyterm=Sonora
Parâmetros streaming (query):
  encoding=linear16, sample_rate=<nativo ou 16000>, channels=1
  interim_results=false, endpointing=300
  + mesmos language/punctuate/numerals/keyterm
Auth: Authorization: Token <DEEPGRAM_UUID>
Formato: body binário (batch) ou frames PCM linear16 (WS)
Timeouts: batch 30s; connect WS 3s; drain 5s; session post-hoc 90s
Streaming ou batch: ambos (DeepgramMode)
Fallback:
  - live finish falha → batch REST com WAV completo
  - streaming_final post-hoc: tenta batch primeiro; se falha e PCM disponível, WS com até 2 tentativas
Retry: reconnect WS post-hoc (2 attempts, backoff 250ms*n)
Rate limit: não específico
Status atual: IMPLEMENTADO
```

Exemplo batch sanitizado:

```http
POST https://api.deepgram.com/v1/listen?model=nova-3&language=pt-BR&punctuate=true&numerals=true&paragraphs=false&keyterm=Sonora
Authorization: Token ********-****-****-****-************
Content-Type: audio/wav

<binary wav>
```

## 4.3 Gemini Multimodal (como motor de STT) — NÃO UTILIZADO NO PIPELINE

```text
Nome exibido na interface: Gemini Multimodal (tag “Avaliação de Pronúncia”, evaluationOnly)
Modelo real (avaliação): gemini-3.5-flash
Endpoint: https://generativelanguage.googleapis.com/v1beta/models/gemini-3.5-flash:generateContent?key=***
Arquivo: src-tauri/src/gemini.rs
Status STT: enum TranscriptionEngine::GeminiMultimodal existe, mas
  audio::transcribe_bytes rejeita qualquer engine que não seja Groq/Deepgram:
  "O motor {:?} não está conectado ao pipeline de captura de áudio."
UI: card não selecionável como motor de transcrição (evaluationOnly)
Status atual: PARCIAL / NÃO USADO PARA TRANSCRIÇÃO
```

## 4.4 OpenRouter

```text
Status atual: NÃO EXISTE
```

## 4.5 Dual engine (modo, não motor isolado)

```text
Nome UI: Modo Motor Duplo (Paralelo)
Comportamento: sempre GroqWhisper + DeepgramNova3 em paralelo
Arquivo: audio.rs (stop_capture_inner, run_dual_posthoc)
Status: IMPLEMENTADO
```

---

# 5. Sanitização

## Modelos disponíveis (UI = backend)

| UI | ID serde | Model id API Groq | Reasoning nativo |
|----|----------|-------------------|------------------|
| LLaMA 70B | `llama-70b` | `llama-3.3-70b-versatile` | Não |
| GPT-OSS 20B | `gpt-oss-20b` | `openai/gpt-oss-20b` | Sim |
| GPT-OSS 120B | `gpt-oss-120b` | `openai/gpt-oss-120b` | Sim |
| Qwen 3.6 27B | `qwen3-27b` | `qwen/qwen3.6-27b` | Não |

Default: `Llama70b`.

## Seleção

- UI Configurações › Motores › Validador Semântico.
- Persistido: `settings.json` + `AppState.sanitizer`.
- Toggle `sanitizer_enabled` (default **true**).

## Prompt

- Base: `settings::DEFAULT_SYSTEM_PROMPT` (constante longa em `settings.rs`).
- Persistido em `settings.json` como `system_prompt`; auto-upgrade se faltar marker `"HowMeia" → "Sonora"`.
- Comandos `get_system_prompt` / `save_system_prompt` existem no backend.
- **UI para editar o system prompt: NÃO ENCONTRADA** (só API IPC).

## Montagem final do system prompt (`groq::call_sanitizer_api`)

1. Base (possivelmente + instrução dual append em `finalize_transcription`)
2. Se `custom_words` não vazio → bloco `--- GLOSSÁRIO PESSOAL DO USUÁRIO ---`
3. User message fixo:

```text
[WHISPER_RAW]: ...
[DEEPGRAM_RAW]: ...
```

## Formato esperado da resposta

- **Texto puro** final (não JSON).
- Sentinel exato: `[FALLBACK_RETRY]` → usa raw acústico.
- Instruções proíbem diálogos, aspas, notas, glossário na saída.
- **Risco residual:** modelo ainda pode devolver explicações/cabeçalhos (mitigado por prompt + temperatura 0; sem parser estruturado que rejeite formato).

## Extração

- `choices[0].message.content` trim.
- Opcional `reasoning` se `include_reasoning=true` (só GPT-OSS com reasoning on).
- Sem schema JSON.

## Parâmetros

```text
temperature: 0.0
reasoning_effort: low|medium|high (default medium) — só se enabled && supports_reasoning
include_reasoning: true nas mesmas condições
max tokens: NÃO definido no request
timeout: 30s (mesmo client HTTP do Whisper)
retries: NÃO
fallback: pick_raw_acoustic / coalesce_empty_final / raw se sem chave Groq
```

---

# 6. Vocabulário específico

```text
Onde salva: settings.json → custom_words: string[]
            + AppState.custom_words
Formato: lista simples de strings
Aliases: NÃO
Categorias: NÃO
Termos rígidos: glossário embutido no DEFAULT_SYSTEM_PROMPT (ChatGPT, Claude, Sonora, etc.)
               + custom_words no final do system prompt
Uso Whisper: NÃO (não há prompt/hotwords Whisper)
Uso Deepgram: apenas keyterm fixo "Sonora" (não usa custom_words)
Uso Gemini avaliação: NÃO
Uso sanitizer: SIM — substituições decididas pelo modelo (conservador)
Substituições automáticas determinísticas: NÃO (só LLM)
```

Exemplo real de estrutura (sem dados sensíveis):

```json
{
  "custom_words": ["Sonora", "Kubernetes", "PostgreSQL"]
}
```

UI: Configurações › Vocabulário — add/remove chips, Enter para adicionar, dedupe case-insensitive no backend.

---

# 7. Histórico

## Onde

- `%APPDATA%\com.haumeavoice.app\history.json`
- Áudio: `%APPDATA%\com.haumeavoice.app\audio\{id}.{ext}`

## Estrutura (`HistoryEntry`)

```text
id: String                    // millis UTC
date: String                  // "YYYY-MM-DD HH:MM" (algoritmo civil em audio.rs; UTC-based)
words: usize
engine: String                // "GroqWhisper" | "DeepgramNova3" | "Groq+Deepgram" | Debug {:?}
text: String                  // final pós-sanitizer
audio_path: Option<String>
evaluation: Option<String>    // Markdown Gemini
duration_ms: u64              // 0 em uploads
source: String                // "mic" | "file"
latency_ms: u64               // total (STT + sanitizer)
throughput: f64               // sanitizer tok/s estimado
transcription_latency_ms, sanitizer_latency_ms: Option<u64>
transcription_throughput, sanitizer_throughput: Option<f64>
realtime_factor: Option<f64>  // STT_ms / duration_ms
deepgram_mode: Option<String> // "batch" | "streaming_final"
total_tokens: Option<usize>   // words * 1.3 arredondado
is_error: Option<bool>
error_message: Option<String>
debug_info: Option<SanitizerDebug>
```

## Comportamento UI

| Ação | Suporte |
|------|---------|
| Abrir/listar | Sim (cards) |
| Pesquisar | Sim (filtra `text` e `error_message`) |
| Copiar texto | **Não** há botão dedicado |
| Editar texto | **Não** |
| Excluir item | **Não** (só “Limpar Tudo”) |
| Avaliar pronúncia | Sim se `audio_path` |
| Retentar falha | Sim se erro + áudio |
| Limite | 200 textos; 10 áudios recentes |
| Persiste restart | Sim |

---

# 8. Avaliação de pronúncia

## Fluxo

1. Botão **“Avaliar Pronúncia”** no card do Histórico (`HistoricoView`).
2. Backend `evaluate_pronunciation(id)`:
   - Carrega entry + `audio_path`
   - Lê bytes (`audio_store::read`)
   - Exige chave Google
   - `gemini::evaluate_pronunciation(bytes, ext, transcript, key)`
3. Modelo: **`gemini-3.5-flash`** — API **direta** Google (`generativelanguage.googleapis.com`), **não** OpenRouter.
4. Prompt completo: função `build_prompt` em `gemini.rs` (estrutura CEFR fixa em Markdown).
5. Áudio: base64 `inline_data` + MIME por extensão.
6. Timeout: **120 s**.
7. Resposta: Markdown com seções `## Resumo Executivo`, `## Placar`, Forças, Pronúncia, Fluência, CEFR, etc.
8. Nota: **subjetiva do modelo** (ex. “Nota geral: X/10”); UI parseia bullets do Placar em `PronunciationEvaluation.tsx`.
9. Persistência: `history::set_evaluation` no mesmo entry.
10. Reutiliza áudio local já salvo; **não** apaga áudio após avaliar.
11. Reabertura: se já tem `evaluation`, botão só expande/oculta (não re-chama API).

## Determinístico vs subjetivo

| Parte | Tipo |
|-------|------|
| Envio áudio+texto, parse HTTP | Determinístico |
| Notas CEFR, fluência, recomendações | **Subjetivo (LLM)** |
| Layout UI a partir de `##` headings | Determinístico (parser local) |
| Fallback se Markdown inválido | Render raw markdown |

---

# 9. Interface atual

## Navegação

`App.tsx` — estado `view: ViewKey`. Sem router URL.

### Início — `src/views/InicioView.tsx`

```text
Função: hub de gravação (timer + botão), stats agregadas do histórico
Estado: IMPLEMENTADO
Problemas conhecidos: atalho exibido fixo Ctrl+B (não lê shortcuts reais); texto final não aparece aqui
```

### Transcrição — `src/views/TranscricaoView.tsx`

```text
Função: upload/drag-drop arquivo → transcribe_file
Estado: IMPLEMENTADO (pipeline)
Problemas conhecidos:
  - Select de idioma é cosmético (não enviado ao backend)
  - Select de modelo desabilitado (“usa motor ativo”)
```

### Histórico — `src/views/HistoricoView.tsx`

```text
Função: lista, busca, clear, avaliar, retry, debug request
Estado: IMPLEMENTADO
Problemas conhecidos: sem edit/copy/delete individual
```

### Atalhos — `src/views/AtalhosView.tsx`

```text
Função: rebind toggle/cancel com captura de teclado
Estado: IMPLEMENTADO
```

### Configurações — `src/views/ConfiguracoesView.tsx`

```text
Abas: Geral | Motores de Nuvem | Vocabulário
Função: autostart, compact gadget, dev mode, mic test, engines, keys, sanitizer, dual, deepgram mode, vocab
Estado: IMPLEMENTADO com ressalvas
Problemas conhecidos:
  - Toggle “Minimizar para bandeja” só local (tray sempre criado no Rust; close já hide-to-tray)
  - Sem tela de “diagnóstico” dedicada (logs em disco; dev mode no histórico)
  - System prompt não editável na UI
```

### Gadget — `src/views/GadgetView.tsx`

```text
Função: overlay idle/recording/transcribing, waveform, drag, hit-rect
Estado: IMPLEMENTADO (Windows sofisticado)
```

### Componentes auxiliares

- `TitleBar.tsx`, `Sidebar.tsx`, `ErrorBoundary.tsx`
- UI kit: `Button`, `Card`, `Input`/`Select`, `Toggle`, `Kbd`
- `PronunciationEvaluation.tsx` — scorecard CEFR

## Estado global

- **Frontend:** React local state por view; sync via `invoke` + `listen`.
- **Backend:** `Arc<AppState>` com RwLock/Mutex.
- Preferências: `settings.json`.
- Cards de motores: seleção chama `update_engine_config` imediatamente.
- Chaves: salvas por botão “Salvar Chave” → `save_api_keys` (substitui trio atômico).

---

# 10. Configurações e credenciais

## Onde ficam as chaves

| Local | Conteúdo |
|-------|----------|
| `%APPDATA%\com.haumeavoice.app\api_keys.json` | groq, google, deepgram (texto plano) |
| RAM `AppState.api_keys` | espelho em runtime |
| `.env` | **NÃO usado** |
| Backend remoto | **NÃO** |

## Criptografia

- **Não.** Comentário explícito em `secrets.rs`: plain text, AppData do usuário.

## Frontend acessa?

- **Sim.** `get_api_keys` devolve as chaves completas para preencher inputs (type password na UI; botão mostrar/ocultar).

## Validação ao salvar

- Groq: prefixo `gsk_`
- Google: prefixo `AIza`
- Deepgram: UUID 8-4-4-4-12

## Teste de chaves

- **Não há** botão “testar chave” dedicado.
- Falha aparece na próxima transcrição/avaliação (histórico com erro).

## Remoção

- Salvar campo vazio → `filter(|s| !s.is_empty())` → `None` no JSON.

## Defaults configuráveis

| Preferência | Default |
|-------------|---------|
| engine | GroqWhisper |
| sanitizer | Llama70b |
| dual_engine | false |
| deepgram_mode | batch |
| sanitizer_enabled | true |
| reasoning_enabled | false |
| reasoning_effort | medium |
| compact_mode | false |
| shortcuts | Control+B / Control+Q |
| custom_words | [] |
| dev_mode | false |

---

# 11. Concorrência e desempenho

| Tópico | Comportamento real |
|--------|-------------------|
| Dual Whisper+Deepgram | **Paralelo** (`tokio::join!`) |
| STT depois sanitizer | **Sequencial** |
| Threads | std::thread para mic start, audio-level, gadget watcher, heartbeat; Tokio para HTTP/WS |
| Live Deepgram | PCM durante gravação; stop só finalize |
| Medição tempo | `Instant` em stop_capture / finalize; campos history |
| HTTP reuse | `OnceLock<reqwest::Client>` em groq/deepgram/gemini |
| Cancelamento gravação | `cancel_capture` + abort live session; **não** cancela HTTP STT já em voo de forma estruturada |
| Timeout | 30s STT/sanitizer; 120s Gemini; WS drains |
| Retry | Deepgram WS post-hoc 2x; histórico “Retentar”; sem retry genérico 429 |
| Fila | Sem fila de jobs; uma gravação por vez (`recording` flag) |
| Rate limit | Não implementado |

## Pontos de latência potenciais

1. Sanitizer LLM (especialmente GPT-OSS + reasoning high)
2. Dual mode limitado pelo motor mais lento
3. Deepgram batch pós-stop (vs live streaming)
4. Resample + clipboard + paste 150 ms sleep
5. Upload de arquivo grande (limite 50 MB)
6. Gemini avaliação (base64 + 120s)

---

# 12. Tratamento de erros

| Situação | Comportamento | O que o usuário vê |
|----------|---------------|-------------------|
| Sem internet | reqwest error | Histórico falha / mensagem rede; sanitizer → raw se STT ok |
| Chave inválida/ausente | Err antes ou HTTP 401/403 | Mensagens PT (“Chave de API do Groq não configurada”, status+body truncado) |
| HTTP 400/500 | ApiError status+body | Card de erro no histórico |
| HTTP 401/403 | idem | idem |
| HTTP 429 | idem (sem backoff) | idem |
| Timeout | client timeout | erro rede/timeout |
| Resposta vazia STT | “Nenhum texto detectado…” | card erro |
| Modelo indisponível | body API | erro; dual degrada se um ok |
| Áudio inválido | API 4xx | erro |
| Sanitizer fora do formato | texto ainda aceito; empty/sentinel → fallback raw | texto bruto no clipboard (mic) |
| Falha sanitização | log + pick_raw | usuário recebe raw |
| Falha exclusão arquivo remoto | N/A (não há delete remoto de áudio) | — |
| Mic falha ao abrir | recording-cancelled | UI volta idle; log |
| Upload >50MB | Err size | toast/erro na Transcrição |
| Gemini sem áudio/key | CommandError | erro sob o card |

---

# 13. Testes existentes

```text
Framework frontend: NENHUM (sem vitest/jest no package.json)
Framework Rust app: testes unitários embutidos no crate
```

### Testes do app (não vendor)

Arquivo: `src-tauri/src/lib.rs` módulo `gadget_placement_tests` — **3 testes**:

- `keeps_a_valid_secondary_monitor_position_and_dpi`
- `recovers_a_position_from_a_disconnected_monitor`
- `clamps_negative_edges_without_forcing_the_primary_monitor`

Comando (não executado nesta análise):

```bash
cd src-tauri && cargo test
```

### Cobertura

- **Aproximada:** mínima (só placement do gadget).
- **Não testado:** audio pipeline, groq, deepgram, gemini, history, UI, IPC.
- **Mocks / APIs reais em CI:** não há.
- Testes em `src-tauri/vendor/quote` são da dependência vendored, **não** do app.

---

# 14. Código legado e dívida técnica

| Item | Classificação | Notas |
|------|---------------|-------|
| `TranscriptionEngine::GeminiMultimodal` no pipeline STT | Parcial / não usado | Enum + UI evaluationOnly; `transcribe_bytes` rejeita |
| Select idioma em TranscricaoView | Não utilizado | Cosmético |
| Toggle tray em GeralTab | Parcial | Estado React não ligado ao backend |
| `save_gadget_position` lógico | Legado | Substituído por physical; ainda no settings |
| System prompt IPC sem UI | Parcial | editável só via invoke manual |
| Duplicação finalize vs retry sanitizer | Dívida | Lógica sanitizer repetida em `retry_transcription_handler` |
| Throughput tokens `words * 1.3` | Heurística | Não é token real da API |
| `now_timestamp` em UTC rotulado como local | Dívida | Pode deslocar data exibida |
| Chaves plain text + expostas ao frontend | Risco segurança local | Documentado no código |
| Acoplamento audio.rs (~2k linhas) | Dívida | orquestra STT+sanitize+history+clipboard |
| Lógica de API na UI | Baixo | UI chama invoke; pouca montagem de request no React |
| Dependência `Otimization.txt` | Documentação paralela | Não executável |
| Patch `vendor/quote` | Work-around | Kaspersky false positive |

**Locais frágeis para mudança de arquitetura de transcrição:**

- `src-tauri/src/audio.rs` (`stop_capture_inner`, `finalize_transcription`, `transcribe_bytes`)
- `src-tauri/src/models.rs` (enums engine/sanitizer)
- `src/views/ConfiguracoesView.tsx` (cards/modos)
- `src/lib/tauri.ts` (tipos IPC)

---

# 15. Pontos de integração para os novos modos

Modos desejados (ainda **não** implementados):

```text
⚡ Ultrarrápido          → Whisper
🚀 Rápido e preciso     → Gemini com áudio
🎯 Preciso              → Whisper + Gemini com áudio
💎 Ultrapreciso         → Whisper + sanitizador + Gemini com áudio
```

## Onde encaixar com mais segurança

| Peça | Caminho sugerido |
|------|------------------|
| Enum / seleção de “modo” | `models.rs` + `settings.rs` + `EngineConfigPayload` + `ConfiguracoesView` |
| Orquestração de etapas | **Extrair** de `audio.rs` um módulo `pipeline` / `transcription_modes` chamado por `stop_capture_inner` e `transcribe_file_path` |
| Whisper existente | Reutilizar `groq::call_whisper_api` |
| Sanitizer existente | Reutilizar `groq::call_sanitizer_api` + flags |
| Gemini áudio STT | **Novo** em `gemini.rs` (hoje só avaliação) ou módulo irmão; **não** reutilizar `evaluate_pronunciation` sem separar prompts |
| Dual Whisper+Deepgram atual | Decícil mapear 1:1 aos 4 modos; decidir se Deepgram permanece paralelo ou vira opcional |
| UI modos | Substituir/estender cards de motor em `MotoresTab` |
| Histórico labels | `history_engine_label` / campo `engine` |

## Componentes reutilizáveis

- `create_wav_buffer` / `audio_store`
- `finalize_transcription` (clipboard/history) — generalizar inputs
- `SanitizerDebug` / dev mode
- `pick_raw_acoustic` / fallbacks

## Partes a extrair antes da mudança

1. Stage1 STT pluggable (trait ou match limpo por modo)
2. Stage2 sanitize opcional por modo
3. Stage “Gemini audio refine” novo
4. Separar avaliação de pronúncia de qualquer STT Gemini

## Riscos

- Gemini STT + avaliação no mesmo modelo/arquivo → prompts misturados
- Dual Deepgram atual vs novos modos (latência/custo/chaves)
- Sanitizer ainda exige chave Groq mesmo com STT só Gemini
- Live Deepgram acoplado a `start_capture` (irrelevante para modos só Whisper/Gemini, mas não deve quebrar)
- Histórico/métricas assumem Whisper/Deepgram slots

## Dependências entre etapas (futuro)

```text
Ultrarrápido:     audio → Whisper → [opcional raw out]
Rápido preciso:   audio → Gemini(audio) → out
Preciso:          audio → Whisper → Gemini(audio|texto?) → out
Ultrapreciso:     audio → Whisper → Sanitizer → Gemini → out
```

(Contrato exato de “Gemini com áudio” ainda é decisão de produto — ver §16.)

---

# 16. Questões que precisam de decisão

1. **Deepgram:** manter como motor/modo paralelo, rebaixar a fallback, ou remover da UI?
2. **Gemini STT:** API direta (como avaliação) vs outro gateway? Qual modelo e prompt de transcrição?
3. **Modos vs motores atuais:** os 4 modos substituem dual_engine + engine select, ou coexistem?
4. **Sanitizer padrão** no modo Ultrapreciso e nos demais (on/off por modo?).
5. **Falha parcial:** se Gemini falha após Whisper ok — entregar Whisper, erro total, ou retry?
6. **Formato de áudio persistido:** manter só WAV 16 kHz mono, ou guardar nativo/FLAC?
7. **Vocabulário:** continuar só no sanitizer ou também hotwords Deepgram / prompt Gemini?
8. **Chaves:** continuar plain text em `api_keys.json` ou migrar para credential manager Windows?
9. **System prompt:** expor editor na UI ou congelar defaults versionados?
10. **Privacidade/telemetria:** métricas só locais bastam, ou há requisito de opt-in remoto?
11. **Idioma:** seletor da Transcrição deve passar a controlar STT (`language=pt-BR` já fixo no Deepgram)?
12. **Gemini Multimodal no enum STT:** remover do `TranscriptionEngine` ou implementar de verdade?

---

# 17. Resumo final obrigatório

## Estado atual

Sonora 1.0.3 é um app **Tauri 2 + React + Rust** para Windows que grava o microfone (ou aceita arquivo), envia áudio a **Groq Whisper** e/ou **Deepgram Nova-3**, opcionalmente **sanitiza** com LLMs Groq, cola o texto no app focado e guarda histórico/áudio localmente. Há overlay gadget sofisticado, atalhos globais, vocabulário no sanitizer e avaliação de pronúncia via **Gemini 3.5 Flash**. Não há OpenRouter. Gemini **não** transcreve no pipeline atual. Testes automatizados do app são quase só placement do gadget.

## O que já funciona

| Recurso | Estado | Arquivos |
| ------- | ------ | -------- |
| Gravação mic + WAV 16k mono | Implementado | `audio.rs`, `mic_control.rs` |
| Atalhos globais + rebind | Implementado | `shortcuts.rs`, `AtalhosView.tsx` |
| Groq Whisper STT | Implementado | `groq.rs` |
| Deepgram batch + live streaming | Implementado | `deepgram.rs` |
| Dual engine paralelo | Implementado | `audio.rs` |
| Sanitizer Groq multi-modelo | Implementado | `groq.rs`, `settings.rs`, `ConfiguracoesView.tsx` |
| Vocabulário custom no sanitizer | Implementado | `settings.rs`, `VocabularioTab` |
| Clipboard + auto-paste | Implementado | `audio.rs` (arboard/enigo) |
| Histórico JSON + áudio | Implementado | `history.rs`, `audio_store.rs`, `HistoricoView.tsx` |
| Retry falhas | Implementado | `retry_transcription`, Histórico |
| Avaliação pronúncia Gemini | Implementado | `gemini.rs`, `PronunciationEvaluation.tsx` |
| Gadget overlay + tray | Implementado | `lib.rs`, `GadgetView.tsx` |
| API keys persistidas | Implementado | `secrets.rs`, MotoresTab |
| Upload arquivo áudio | Implementado | `TranscricaoView.tsx`, `transcribe_file` |
| Dev mode debug sanitizer | Implementado | `SanitizerDebug`, Histórico |
| Logs + crash/AppHang diagnostics | Implementado | `lib.rs` |
| Autostart Windows | Implementado | `commands.rs` get/set_autostart |
| Teste de microfone UI | Implementado | `audio.rs`, GeralTab |

## O que está incompleto

| Recurso | Problema | Impacto |
| ------- | -------- | ------- |
| Gemini como STT | Enum/UI existem; pipeline rejeita | Não dá para “Gemini com áudio” sem código novo |
| OpenRouter | Ausente | — |
| Idioma na Transcrição | UI não ligada | Usuário acha que controla idioma |
| Toggle bandeja | Só estado React | Preferência ilusória |
| Editor system prompt | IPC sem tela | Só via defaults/auto-upgrade |
| Edit/copy/delete histórico item | Não implementados | UX limitada |
| Testes pipeline/API | Quase zero | Refactor arriscado |
| Criptografia de chaves | Plain text + frontend | Risco em máquina compartilhada |
| Cancel HTTP in-flight | Não estruturado | Stop/cancel não aborta request já enviada |

## Arquivos mais importantes

1. `src-tauri/src/audio.rs`
2. `src-tauri/src/groq.rs`
3. `src-tauri/src/deepgram.rs`
4. `src-tauri/src/gemini.rs`
5. `src-tauri/src/models.rs`
6. `src-tauri/src/commands.rs`
7. `src-tauri/src/lib.rs`
8. `src-tauri/src/settings.rs`
9. `src-tauri/src/history.rs`
10. `src-tauri/src/secrets.rs`
11. `src-tauri/src/audio_store.rs`
12. `src-tauri/src/shortcuts.rs`
13. `src/lib/tauri.ts`
14. `src/views/ConfiguracoesView.tsx`
15. `src/views/HistoricoView.tsx`
16. `src/views/InicioView.tsx`
17. `src/views/TranscricaoView.tsx`
18. `src/views/GadgetView.tsx`
19. `src/components/PronunciationEvaluation.tsx`
20. `src-tauri/tauri.conf.json`

## Fluxo atual real

```text
[Atalho/Botão]
  → handle_toggle
  → cpal buffer (+ Deepgram live?)
  → stop → WAV 16k mono
  → save audio disk
  → STT: Whisper and/or Deepgram
  → Sanitizer Groq? (on/off)
  → clipboard + Ctrl+V
  → history.json + event UI
```

## Riscos antes da mudança

1. Toda orquestração concentrada em `audio.rs` — mudanças grandes sem extrair módulos quebram mic e upload juntos.
2. Dual Deepgram + live session acoplados ao start da gravação.
3. Sanitizer e Whisper compartilham a **mesma chave Groq**.
4. Gemini hoje é só avaliação; reutilizar o mesmo módulo sem separar prompts pode corromper o Histórico de pronúncia.
5. Pouquíssimos testes automatizados.
6. UI e backend divergem em alguns toggles (bandeja, idioma).
7. Expectativa de “4 modos” não mapeia 1:1 para engine+dual+deepgram_mode atuais.

## Informações ainda desconhecidas

- Comportamento em runtime com chaves reais / latências típicas do usuário (análise só estática).
- Conteúdo real de `settings.json` / histórico na máquina do usuário (não lido de propósito).
- Se `gemini-3.5-flash` está disponível na conta Google do usuário (string hardcoded).
- Cobertura de código medida por ferramenta (não executada).
- Conteúdo operacional completo de `Otimization.txt` além do que o código já implementou.
- Políticas de conta/limites Groq/Deepgram/Google do projeto.

---

## Metadados da entrega

| Campo | Valor |
|-------|-------|
| Arquivo | `APP_CONTEXT_SONORA.md` (raiz do repositório) |
| Arquivos de app analisados | 35 |
| Testes/comandos executados | Nenhum (`cargo test` / build / APIs **não** rodados) |
| Limitações | Análise estática apenas; sem execução do app; segredos não lidos; vendor/target ignorados |
