# Auditoria da implementação do Gemini — Sonora

**Data:** 2026-07-18  
**Escopo:** somente leitura de código (sem chamadas de API, sem alteração de lógica de app)  
**Artefato:** este relatório  
**Skills consultadas:** `impeccable` (contexto de produto/UI), `copywriting` (clareza do relatório), `tauri` (IPC/desktop)

---

## Fontes principais

| Área | Caminho |
|------|---------|
| Cliente HTTP / modelo / timeouts | `src-tauri/src/gemini/client.rs` |
| Files API (upload / poll / delete) | `src-tauri/src/gemini/files.rs` |
| STT Gemini | `src-tauri/src/gemini/transcription.rs` |
| Refine / pure STT em arquivo remoto | `src-tauri/src/gemini/refinement.rs` |
| Pronúncia (inline Base64) | `src-tauri/src/gemini/pronunciation.rs` |
| Prompts versionados | `src-tauri/src/gemini/prompts.rs` |
| Orquestração dos modos | `src-tauri/src/transcription/modes.rs` |
| Sanitizer (Groq) | `src-tauri/src/transcription/pipeline.rs`, `src-tauri/src/groq.rs` |
| Captura / WAV / clipboard | `src-tauri/src/audio.rs` |
| Vocabulário / literais | `src-tauri/src/vocabulary.rs` |
| UI de métricas | `src/views/HistoricoView.tsx` |

---

## 1. Fluxo completo

### 1.1 Rápido e preciso (`run_fast_accurate`)

**Arquivo:** `src-tauri/src/transcription/modes.rs` → `transcribe_audio` → `upload_and_wait` + `generate_content` + `cleanup`

```text
áudio finalizado (mic: PCM → resample 16 kHz → WAV em RAM)
→ preparação (mime/ext, chave Google, flag fallback)
→ upload Files API (start resumable + bytes finalize)
→ polling até ACTIVE (800 ms)
→ geração generateContent (prompt STT + file_data)
→ parsing (candidates[].content.parts[].text)
→ exclusão do arquivo remoto (await cleanup)
→ [opcional] fallback Whisper se Gemini falhar/vazio e flag ligada
→ clipboard + paste (só mic)
→ histórico (mode_result_to_history)
```

| Etapa | Existe? |
|-------|---------|
| preparação | sim |
| upload | sim (Files API) |
| polling | sim |
| geração | sim |
| parsing | sim |
| exclusão | sim (antes de devolver texto) |
| Base64 | **não** |
| Whisper no caminho feliz | **não** |
| sanitizador | **não** |
| clipboard | sim (mic) |
| histórico | sim |

---

### 1.2 Preciso (`run_precise`)

**Arquivo:** `modes.rs` + `refine_precise_with_file` / `transcribe_with_file`

```text
áudio finalizado
→ preparação (glossário, chave Google obrigatória)
→ PARALLEL:
     ├─ Whisper (Groq) em clone do WAV
     └─ upload_and_wait (mesmo WAV)
→ se Whisper ok + upload ok:
     → generateContent refine (áudio + hipótese Whisper + glossário)
     → parsing
     → exclusão (cleanup)
     → apply_strict_literals
→ se Whisper vazio/falha mas upload ok:
     → generateContent pure STT no arquivo
     → exclusão
→ se upload falha e Whisper ok:
     → entrega Whisper (fallback)
→ clipboard + histórico
```

| Etapa | Existe? |
|-------|---------|
| Whisper ∥ upload | sim (`tokio::join!`) |
| sanitizador | **não** |
| refine Gemini | sim (caminho feliz) |
| pure Gemini | sim (Whisper falhou) |
| exclusão síncrona | sim (`guard.cleanup().await` antes do return) |
| strict literals | sim (pós-Gemini) |

---

### 1.3 Ultrapreciso (`run_ultra_precise`)

```text
áudio finalizado
→ preparação (glossário, chave Google opcional no refine)
→ PARALLEL:
     ├─ Whisper
     └─ upload_and_wait
→ SE Whisper falhar/vazio → erro (cleanup upload se ok)
→ sanitizador Groq sequencial (run_sanitize; Whisper só; Deepgram "")
→ detect_content_type(Whisper) → nota de conteúdo
→ SE upload ok + chave:
     → refine_ultraprecise_with_file (áudio + Whisper + sanitizado + glossário + content_note)
     → parsing
     → exclusão
→ SENÃO / Gemini falha:
     → entrega sanitizado (ou Whisper se sanitizer raw fallback)
→ apply_strict_literals
→ clipboard + histórico
```

| Etapa | Existe? |
|-------|---------|
| Whisper ∥ upload | sim |
| sanitizador | sim (após Whisper; sequencial) |
| Gemini refine | sim |
| exclusão síncrona | sim |
| telemetria sanitizer no histórico | **quebrada** (sempre 0 ms — ver §5) |

---

### 1.4 Avaliação de pronúncia (`evaluate_pronunciation`)

**Arquivo:** `src-tauri/src/gemini/pronunciation.rs`  
**Entrada UI:** `commands::evaluate_pronunciation` (áudio salvo no card do Histórico)

```text
usuário clica “Avaliar pronúncia”
→ leitura do arquivo de áudio local
→ Base64 STANDARD do áudio inteiro
→ generateContent (prompt CEFR + inline_data)
→ parsing Markdown
→ grava evaluation no HistoryEntry
```

| Etapa | Existe? |
|-------|---------|
| Files API | **não** |
| upload / poll / delete | **não** |
| Base64 inline | **sim** |
| STT / sanitizer / clipboard | **não** (fluxo separado) |

---

## 2. Forma de envio do áudio

### 2.1 STT / refine (modos de produto)

| Pergunta | Resposta objetiva | Evidência |
|----------|-------------------|-----------|
| Inline Base64 ou Files API? | **Files API** (resumable) | `files.rs`, `transcription.rs`, `refinement.rs` |
| Formato mic | WAV RIFF mono 16-bit | `audio.rs` `create_wav_buffer` |
| Sample rate | **16 000 Hz** alvo | `TARGET_SAMPLE_RATE = 16_000` |
| MIME mic | `audio/wav` | `run_product_mode(..., "audio/wav", "wav")` |
| MIME upload arquivo | por extensão (`mime_for_ext`) | `client.rs` |
| Tamanho típico (áudio curto ~3–5 s) | ~96–160 KB WAV 16 kHz mono 16-bit | \(rate × 2 bytes × segundos\) |
| Conversão antes do envio | resample nativo → 16 kHz se necessário; encode WAV | `audio.rs` stop path |
| Base64 no STT/refine | **não** | só `pronunciation.rs` |
| Base64 gerado mais de uma vez | só na pronúncia (1× por avaliação) | — |
| Cópias de memória | **sim:** `audio.clone()` para Whisper paralelo; `bytes.to_vec()` no body do upload | `modes.rs`, `files.rs:233` |

### 2.2 Files API — detalhe

| Item | Valor | Onde |
|------|-------|------|
| Start upload | `POST https://generativelanguage.googleapis.com/upload/v1beta/files?key=…` | `start_resumable_upload` |
| Headers start | `X-Goog-Upload-Protocol: resumable`, `Command: start`, Content-Length/Type | idem |
| Finalize | `POST {X-Goog-Upload-URL}` com bytes brutos | `upload_bytes` |
| URI | campo `uri` da resposta; fallback `{API_ROOT}/{name}` | `upload_and_wait` |
| Poll | `GET {API_ROOT}/{name}?key=…` | `get_file` / `wait_until_active` |
| Intervalo | **800 ms** (`POLL_INTERVAL`) | `client.rs:21` |
| Pronto quando | `state == "ACTIVE"` **ou** (state vazio **e** `uri` presente) | `files.rs:301–305` |
| Max polls | implícito: até **90 s** wall (`TIMEOUT_FILE_READY`) ≈ ≤ ~112 polls | `client.rs:20` |
| Timeout upload | 60 s por request | `TIMEOUT_UPLOAD` |
| Timeout poll GET | 15 s por GET | `TIMEOUT_POLL` |
| Timeout generate | 120 s | `TIMEOUT_GENERATE` |
| Timeout delete | 15 s | `TIMEOUT_DELETE` |
| Geração | `POST …/v1beta/models/gemini-3.5-flash:generateContent?key=…` | `generate_url` |
| Exclusão | **após** generate, com `await cleanup()` no caminho feliz; Drop best-effort se panicar | `transcription.rs:46`, `modes.rs` |
| Delete falha | log warn; **não** falha a transcrição (erro engolido no cleanup `let _ =`) | `files.rs:90`, `324–363` |

### 2.3 Pronúncia

| Item | Valor |
|------|-------|
| Método | `inline_data` Base64 |
| MIME | `mime_for_ext(ext)` |
| Files API | não |

---

## 3. API utilizada

| Item | Valor |
|------|-------|
| Provedor | **Google AI Studio / Generative Language API direto** |
| OpenRouter | **não** (`gemini/mod.rs` documenta; nenhum host openrouter) |
| API | **`generateContent`** (não Interactions API) |
| Versão | **`v1beta`** |
| Modelo | **`gemini-3.5-flash`** (`GEMINI_MODEL`) |
| Temperatura | **não enviada** (default do modelo) |
| `generationConfig` | **ausente** no body |
| Reasoning / thinking | **não configurado** no Gemini |
| `store` | **não enviado** |
| Timeout generate | 120 s |
| Retries | **nenhum** no cliente Gemini |
| Fallback de modelo Gemini | **nenhum** (só fallback Whisper onde previsto) |
| HTTP client | **reutilizado** (`OnceLock<reqwest::Client>`) |

### Exemplo sanitizado de request (Files path — refine)

```http
POST https://generativelanguage.googleapis.com/v1beta/models/gemini-3.5-flash:generateContent?key=***
Content-Type: application/json
```

```json
{
  "contents": [
    {
      "parts": [
        {
          "text": "Você é o revisor final…\nHipótese Whisper:\n\"\"\"\n…texto…\n\"\"\"\nGlossário do usuário:\n- Sonora [application] (aliases: …) [LITERAL]\n"
        },
        {
          "file_data": {
            "mime_type": "audio/wav",
            "file_uri": "https://generativelanguage.googleapis.com/v1beta/files/xxxxx"
          }
        }
      ]
    }
  ]
}
```

### Exemplo sanitizado (pronúncia — inline)

```json
{
  "contents": [{
    "parts": [
      { "text": "Analise o áudio como um avaliador…\nTranscrição…\n\"\"\"\n…\n\"\"\"" },
      {
        "inline_data": {
          "mime_type": "audio/wav",
          "data": "<BASE64_OMITIDO>"
        }
      }
    ]
  }]
}
```

---

## 4. Paralelismo

### Preciso e Ultrapreciso

| Pergunta | Resposta |
|----------|----------|
| Whisper e upload começam juntos? | **Sim** — `tokio::join!(whisper_fut, upload_fut)` |
| Sanitizer só após Whisper? | **Sim** (UltraPrecise); Precise **não tem** sanitizer |
| Gemini aguarda Whisper **e** sanitizer? | UltraPrecise: **sim** (precisa dos dois textos + arquivo ACTIVE) |
| Base64 em paralelo? | N/A nos modos STT (não usa Base64) |
| Etapas que poderiam ser paralelas e não são | UltraPrecise: sanitizer **espera** join completo mesmo se Whisper já terminou antes do upload; não há overlap sanitizer∥resto do upload se Whisper acabar cedo *dentro* do join o upload já corre — mas se Whisper ≫ upload, sanitizer só começa depois do join (ok). Se upload ≫ Whisper, **sanitizer não começa até o upload terminar** ← **sequência evitável** |

### Timeline aproximada (áudio curto, rede típica, Preciso feliz)

```text
0 ms     — gravação termina; WAV 16 kHz pronto
0–50 ms  — save local + spawn pipelines
0 ms     — Whisper e upload começam (paralelo)
~800–2500 ms — Whisper termina (Groq; variável)
~500–4000 ms — upload start+bytes+poll ACTIVE (pode exigir 1–N × 800 ms sleep)
max(W,U) — join termina
+1500–6000 ms — generateContent multimodal
+100–800 ms  — delete remoto (await)
+strict + clipboard
≈ 7–10 s total  — compatível com (upload+poll) + generate + delete
```

### Rápido e preciso (sequencial puro)

```text
0 ms     — início
…        — upload + poll (sem Whisper paralelo)
…        — generate
…        — delete
≈ 7–10 s — quase todo o tempo no Gemini Files + generate
```

---

## 5. Sanitizador no Ultrapreciso — por que “0,00 s sanitizador”

### O sanitizador **é executado** no código

```760:765:src-tauri/src/transcription/modes.rs
    let ts = std::time::Instant::now();
    let sanitize =
        crate::transcription::run_sanitize(state, &whisper_text, "", false).await;
    let sanitizer_ms = ts.elapsed().as_millis() as u64;
    stages.push(format!("sanitizer:{}ms", sanitizer_ms));
```

Condições em `run_sanitize` (`pipeline.rs`):

- se `sanitizer_enabled == false` → pick_raw, **sem** chamada Groq (latência ~0 real);
- se sem chave Groq → raw fallback;
- se com chave → `call_sanitizer_api` e preenche `debug_info`.

O tempo real fica em `stages` (ex.: `sanitizer:1234ms`) e dentro do **total** `transcription_latency_ms`.

### O histórico **não grava** esse tempo

```588:591:src-tauri/src/transcription/modes.rs
        latency_ms: result.transcription_latency_ms,
        ...
        transcription_latency_ms: Some(result.transcription_latency_ms),
        sanitizer_latency_ms: Some(0),
```

**Sempre `Some(0)`** para todos os modos de produto, inclusive UltraPrecise.

### UI

```389:393:src/views/HistoricoView.tsx
{(h.transcription_latency_ms / 1000).toFixed(2)}s motor + {(h.sanitizer_latency_ms / 1000).toFixed(2)}s sanitizador
```

Portanto:

| Observação UI | Interpretação real |
|---------------|--------------------|
| `0,00 s sanitizador` | **campo histórico hardcoded 0**, não prova que o sanitizer não rodou |
| `X,XX s motor` | **pipeline inteiro** (Whisper + upload + sanitizer + Gemini + delete), não só “motor acústico” |
| Tempo do sanitizer | está **dentro do motor** + opcionalmente em `stages` / Request debug |

### Fallback silencioso possível

- `sanitizer_enabled` off → raw Whisper, warnings, latência ~0 real **e** UI 0.
- parse/API fail → `used_raw_fallback`, ainda assim `sanitizer_latency_ms` histórico = 0.
- UltraPrecise continua e manda “sanitizado” (= Whisper se fallback) ao Gemini.

---

## 6. Telemetria atual

### Métricas

| Métrica | Status | Onde |
|---------|--------|------|
| Duração do áudio (`duration_ms`) | real | samples / capture_rate |
| Total pipeline modos (`transcription_latency_ms`) | real (wall clock do modo) | `t0` em cada `run_*` |
| `latency_ms` | = total modos | history |
| `sanitizer_latency_ms` (modos) | **incorreto (sempre 0)** | `mode_result_to_history` |
| `sanitizer_latency_ms` (legado) | real | `pipeline.rs` SanitizeOutcome |
| Whisper ms | real (log + stages) | Precise/UltraPrecise |
| Upload ms | real (log + stages; **inclui poll**) | Precise/UltraPrecise |
| Gemini generate ms | real (stages; **não** inclui upload no Precise path com arquivo pré-upado) | `gemini_*` stages |
| FastAccurate `gemini_ms` | **agrupado** = wall total do modo (upload+poll+gen+delete) | `gemini_ms: Some(ms)` com `ms = t0.elapsed()` |
| Delete ms | **não medido** | cleanup await sem timer |
| Base64 ms | **não medido** (só pronúncia) | — |
| Preparação / resample | log de tamanho; **sem** campo history | `audio.rs` |
| Clipboard | timeout 3 s; **sem** métrica history | `deliver_clipboard_and_paste` |
| Poll count / poll ms separado | **não** (só dentro de upload_ms) | — |
| Throughput tok/s | **estimado** (`est_throughput`) | telemetry |
| RTF | real (latência / duração áudio) | — |
| Stages string | real concatenado | history.stages |
| Breakdown UI motor/sanitizer | **legado de dois estágios**; modos forçam sanitizer=0 | HistoricoView |

### Por que só “motor” e “sanitizador”

A UI foi desenhada para o pipeline legado:

1. motor acústico (`transcription_latency_ms`)
2. sanitizer Groq (`sanitizer_latency_ms`)

Os modos de produto **reutilizaram os mesmos campos** sem mapear upload/poll/Gemini/delete. Resultado: tudo vira “motor”; sanitizer aparece zerado.

---

## 7. Latência (7–10 s) — classificação por contribuição

| Etapa | Contribuição esperada | Notas |
|-------|----------------------|-------|
| `generateContent` multimodal com áudio | **alta** | principal custo de modelo; timeout até 120 s |
| Polling Files API (800 ms × N) | **alta / média** | mínimo 0 se já ACTIVE; senão soma 0.8 s por ciclo; até 90 s |
| Upload resumable (2 HTTP) | **média** | start + finalize; áudio curto é leve, RTT domina |
| Delete síncrono antes de entregar texto | **média / baixa** | 1 RTT extra na critical path (`cleanup().await`) |
| Whisper Groq (Precise/Ultra) | **média** | paralelo ao upload; no critical path só se Whisper > upload |
| Sanitizer Groq (Ultra) | **média** | **serial** após join; pode adicionar 0.5–3 s |
| Ultra: sanitizer bloqueado até upload acabar | **média** | se Whisper rápido e upload lento, sanitizer atrasa sem necessidade |
| Resample / WAV encode | **baixa** | CPU local |
| Cópias `audio.clone` / `to_vec` | **baixa** | RAM, não 7–10 s |
| Cliente HTTP novo a cada request | **baixa** | client é singleton reutilizado |
| Base64 STT | **n/a** | não usado nos modos |
| Reasoning Gemini | **desconhecida / provavelmente n/a** | não há config de thinking |
| Prompts grandes | **baixa / média** | glossário + drafts; menor que áudio multimodal |
| Retries | **n/a** | não há |
| Sleeps fixos | **média** | só `POLL_INTERVAL` 800 ms |

### Causa mais provável do sintoma 7–10 s

Soma de:

1. **round-trips Files API** (start + upload + ≥1 poll),  
2. **`generateContent` com áudio**,  
3. **`delete` síncrono na critical path**,  
4. no UltraPrecise: **+ sanitizer serial** (tempo real escondido dentro de “motor”).

Áudio curto **não** reduz muito (1)+(2): o overhead de Files + cold generate domina sobre segundos de áudio.

---

## 8. Vocabulário e “Sonora” → “Homey Voice”

### Como o glossário chega ao Gemini

1. `format_glossary_for_prompt` (`vocabulary.rs`) monta linhas:

   ```text
   - Sonora [application] (aliases: …) [LITERAL]
   ```

2. Injetado nos prompts Precise / UltraPrecise (`precise_refinement_prompt`, `ultraprecise_refinement_prompt`).
3. **Não** entra no STT puro do Rápido e preciso (`transcription_prompt` sem glossário).
4. Pós-processamento: `apply_strict_literals` só para termos `enabled && strict`.

### Por que pode virar “Homey Voice”

| Mecanismo | Efeito |
|-----------|--------|
| Whisper / Gemini erram foneticamente | “Sonora” → “Homey” é confusão acústica plausível |
| Prompt pede fidelidade ao **áudio** | se o modelo “ouve” Homey, pode preferir isso ao glossário |
| `[LITERAL]` no prompt | instrução **soft**; modelo pode ignorar |
| `apply_strict_literals` | só substitui se achar o **alias ou canônico** no texto final |
| Se saída = `Homey Voice` e aliases **não** incluem `Homey` / `Homey Voice` | **nenhuma correção determinística** |
| Strict no canônico | normaliza casing de “sonora”, **não** inventa match fonético |

Conclusões:

- `Sonora` **pode** estar no prompt (se cadastrado e enabled).
- Literal rígido **só ajuda** se (a) o modelo obedecer ou (b) um alias inequívoco aparecer no texto.
- O modelo **pode normalizar livremente** nomes; não há validador semântico pós-Gemini além do replace de aliases strict.
- Rápido e preciso **não envia glossário** → maior risco de “Homey”.

---

## 9. Fallbacks

### Rápido e preciso

| Falha | Comportamento |
|-------|----------------|
| Sem chave Google + fallback off | erro |
| Sem chave + fallback on | Whisper (marcado fallback) |
| Gemini vazio/erro + fallback on | Whisper |
| Gemini vazio/erro + fallback off | erro |
| Delete falha | ignorado; texto ok |
| Literal rígido alterado | **sem** strict pass dedicado no FastAccurate além do `run_product_mode` global strict no final |

### Preciso

| Falha | Comportamento |
|-------|----------------|
| Sem chave Google | erro (sem Whisper-only automático) |
| Refine falha, Whisper ok | **Whisper** + `used_fallback` |
| Whisper falha, upload ok | Gemini pure STT + fallback |
| Ambos falham | erro |
| Upload falha, Whisper ok | Whisper only + fallback |
| Delete falha | ignorado |
| Strict literal | aplicado em vários ramos de sucesso |

### Ultrapreciso

| Falha | Comportamento |
|-------|----------------|
| Whisper falha/vazio | **erro** (não tenta Gemini pure) |
| Sanitizer falha | raw Whisper → segue para Gemini com “sanitizado”=Whisper; stages/warnings |
| Gemini falha | entrega sanitizado; fallback |
| Upload falha | sanitizado; fallback |
| Sem chave Google | sanitizado; fallback |
| Delete falha | ignorado |
| Resposta altera literal | `apply_strict_literals` se alias casar; senão permanece |

### Pronúncia

| Falha | Comportamento |
|-------|----------------|
| Sem chave / áudio vazio / generate erro | erro na UI; não mexe na transcrição |

### Silencioso?

- Delete: **sim** (warn log only).  
- Sanitizer raw fallback: **parcialmente** (stages/warnings; UI sanitizer 0 s mascara).  
- Fallback Whisper/sanitizado: **visível** via `used_fallback` / selos se a UI mostrar.  
- “Homey” por modelo: **silencioso** se não houver alias.

---

## 10. Comparação com a arquitetura planejada

| Plano | Implementação | Status |
|-------|---------------|--------|
| Ultrarrápido = Whisper | `run_ultra_fast` Whisper only | **implementado corretamente** |
| Rápido e preciso = Gemini com áudio | Files API + STT prompt; fallback Whisper opcional | **implementado corretamente** (com fallback extra) |
| Preciso = Whisper + Gemini com áudio | paralelo + refine; pure STT se Whisper falha | **implementado corretamente** |
| Ultrapreciso = Whisper + sanitizer + Gemini | ordem real: (W∥U) → S → G | **parcialmente** — sanitizer **não** paralelo ao upload; telemetria sanitizer **errada** |
| Glossário em todos os modos Gemini | só Precise/UltraPrecise | **diferente do ideal de produto** se esperado no Rápido |
| OpenRouter | não | alinhado ao plano “AI Studio direto” |
| Inline Base64 STT | não (só pronúncia) | alinhado a Files API para STT |

---

## Diagrama de round-trips (caminho feliz Gemini)

### Rápido e preciso — ~4 HTTP Gemini + 0 Groq

```text
1. POST upload/v1beta/files (start)
2. POST upload-url (bytes)
3. GET files/{id}          (× N polls, sleep 800 ms)
4. POST models/…:generateContent
5. DELETE files/{id}       (critical path)
```

### Preciso — Whisper Groq ∥ (1–3) depois 4–5

```text
Groq Whisper ─────────────────────┐
Files 1→2→3 ─────────────────────┴→ generate refine → delete
```

### Ultrapreciso

```text
Groq Whisper ──┐
Files 1→2→3 ───┴→ Groq sanitizer → generate ultraprecise → delete
```

---

### Resumo executivo

* **Método de envio atual:** Gemini Files API (resumable) para STT/refine; Base64 inline **somente** na avaliação de pronúncia. Google Generative Language `v1beta` direto — **sem OpenRouter**.
* **Quantidade de round trips (caminho feliz Gemini):** tipicamente **4–5** HTTP ao Google (start, upload, ≥1 poll, generate, delete) + Whisper/sanitizer Groq conforme o modo.
* **Causa mais provável da latência 7–10 s:** overhead **Files API (upload + polling 800 ms)** + **`generateContent` multimodal** + **delete síncrono** na critical path; no UltraPrecise soma-se o **sanitizer serial** após o join.
* **Sanitizador no UltraPrecise:** **sim, o código executa** `run_sanitize`; o **“0,00 s” é bug de telemetria** (`sanitizer_latency_ms: Some(0)` em `mode_result_to_history`), com o tempo real embutido em “motor” e em `stages`.
* **Problemas de telemetria:** UI legado motor/sanitizer; modos gravam total só em motor; upload/poll/generate/delete não aparecem separados; FastAccurate marca `gemini_ms` como wall total.
* **Problemas de literais:** glossário ausente no Rápido e preciso; `[LITERAL]` é soft no prompt; `apply_strict_literals` só corrige **aliases conhecidos** — “Homey Voice” sem alias não volta para “Sonora”.
* **Três otimizações de maior impacto:**
  1. **Entregar texto ao usuário antes do delete** (cleanup em background / Drop já existe — parar de `await cleanup` na critical path).
  2. **UltraPrecise: iniciar sanitizer assim que Whisper terminar**, sem esperar upload (pipeline em estágios com canais).
  3. **Telemetria real + UI:** gravar `sanitizer_ms` / `upload_ms` / `poll_ms` / `generate_ms` / `delete_ms`; opcionalmente reduzir `POLL_INTERVAL` ou sair no primeiro ACTIVE; avaliar **inline Base64** para áudios curtos (<~X MB) para eliminar poll.
* **Arquivos que precisariam ser alterados (se/quando for otimizar):**
  - `src-tauri/src/transcription/modes.rs` (paralelismo Ultra, history fields)
  - `src-tauri/src/gemini/files.rs` / `client.rs` (poll, timeouts)
  - `src-tauri/src/gemini/transcription.rs` / `refinement.rs` (ordem cleanup)
  - `src-tauri/src/gemini/prompts.rs` + `vocabulary.rs` (literais / glossário no FastAccurate)
  - `src/views/HistoricoView.tsx` (métricas por estágio)
  - opcional: `src-tauri/src/models.rs` (novos campos de history)

---

*Fim da auditoria. Nenhum código de produto foi modificado nesta tarefa além da criação deste documento solicitado.*
