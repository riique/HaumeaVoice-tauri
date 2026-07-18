# Plano de migração — Modos de transcrição

Fase 00 (preparação). **Sem alteração funcional do app nesta fase.**

**Alvo de produto (modos futuros):**

```text
⚡ Ultrarrápido      → Whisper
🚀 Rápido e preciso → Gemini com áudio
🎯 Preciso          → Whisper + Gemini com áudio
💎 Ultrapreciso     → Whisper + sanitizador + Gemini com áudio
```

**Restrições permanentes até decisão explícita:**

- Não remover Deepgram
- Preservar atalhos, gadget, clipboard, histórico e avaliação de pronúncia
- Não expor chaves; não chamar APIs reais salvo fase que autorize
- Skills: `tauri` (IPC/Rust/segurança), `impeccable` (UX), `copywriting` (textos UI)

Referências: `APP_CONTEXT_HAUMEA_VOICE.md`, `docs/TRANSCRIPTION_BEHAVIOR_BASELINE.md`

---

## 1. Checkpoint e branch

### Estado do repositório na Fase 00

- Branch inicial: `master` **sem commits** no momento da preparação (working tree 100% untracked).
- Baseline de build: ver `TRANSCRIPTION_BEHAVIOR_BASELINE.md` §1 — `npm run build`, `cargo check`, `cargo test` **OK**.

### Checkpoint local recomendado

```powershell
# Na raiz do projeto, após Fase 00 docs commitados:
git add -A
git status   # revisar: sem .env, sem api_keys, sem AppData
git commit -m "chore: baseline pre-transcription-modes (phase 00)"
git branch chore/transcription-modes
git checkout chore/transcription-modes
```

| Artefato | Propósito |
|----------|-----------|
| Commit baseline na `master` | Snapshot restaurável pré-migração |
| Branch `chore/transcription-modes` | Trabalho das fases 01+ |
| `docs/TRANSCRIPTION_*` | Plano + baseline comportamental |

### Rollback rápido

```powershell
git checkout master
# ou, se já em branch de migração e commit ruim:
git reset --hard <sha-do-baseline>
```

Dados do usuário em `%APPDATA%\com.haumeavoice.app\` **não** estão no git — rollback de código não apaga histórico/chaves locais.

---

## 2. Princípios de migração (Tauri)

1. **Core Process (Rust)** dono da orquestração STT; WebView só seleciona modo e exibe estado.
2. **IPC estável:** novos campos em payloads com `#[serde(default)]` para não quebrar front antigo no meio da migração.
3. **Sem chaves no frontend além do fluxo atual** de settings; nunca logar secrets.
4. **CSP / capabilities:** novos hosts HTTPS só se Gemini/outros endpoints mudarem; atualizar `tauri.conf.json` CSP `connect-src` se o WebView passar a chamar APIs (hoje as APIs são só no Rust — preferir manter assim).
5. **Extrair pipeline de `audio.rs` antes de reescrever modos** — evita big-bang no arquivo P0.
6. **Gemini avaliação ≠ Gemini STT** — arquivos/prompts separados.

---

## 3. Mapa de arquivos por tipo de mudança

### Tocar cedo (extração / contrato)

| Arquivo | Mudança esperada |
|---------|------------------|
| `src-tauri/src/audio.rs` | Extrair orquestração; plugar modos |
| `src-tauri/src/models.rs` | Novo enum `TranscriptionMode` (ou equivalente) |
| `src-tauri/src/settings.rs` | Persistir modo; defaults; migração de settings antigos |
| `src-tauri/src/commands.rs` | `get/update` de modo no IPC |
| `src/lib/tauri.ts` | Tipos espelhando Rust |
| `src/views/ConfiguracoesView.tsx` | UI dos 4 modos (copywriting + impeccable) |

### Reutilizar sem quebrar

| Arquivo | Uso |
|---------|-----|
| `src-tauri/src/groq.rs` | Whisper + sanitizer |
| `src-tauri/src/deepgram.rs` | Manter; fora dos 4 modos até decisão |
| `src-tauri/src/gemini.rs` | Pronúncia intacta; **novo** path STT separado |
| `src-tauri/src/history.rs` / `audio_store.rs` | Terminal do pipeline |
| `src-tauri/src/shortcuts.rs` | Intactos |
| `src/views/GadgetView.tsx` / `InicioView.tsx` | Eventos iguais |

### Não tocar sem necessidade

- `mic_control.rs`, tray/gadget placement em `lib.rs`
- `PronunciationEvaluation.tsx` (só se labels genéricos)
- `vendor/quote`

---

## 4. Fases propostas

| Fase | Nome | Objetivo | Entrega | APIs reais? | UI? |
|------|------|----------|---------|-------------|-----|
| **00** | Preparação | Baseline, plano, checkpoint | Este doc + baseline + builds verdes | Não | Não |
| **01** | Extração de pipeline | Separar Stage1/Stage2/finalize de `audio.rs` sem mudar comportamento | Módulo(s) Rust; mesmos resultados | Não | Não |
| **02** | Contrato de modo | Enum + settings + IPC + tipos TS; default = comportamento atual | Persistência compatível | Não | Mínima/oculta se preciso |
| **03** | Ultrarrápido | Modo = só Whisper (mapear default atual single Groq) | Path explícito | Opcional smoke | Labels |
| **04** | Gemini STT | Implementar “Gemini com áudio” separado da avaliação | `gemini` STT + chave Google | Opcional smoke | Card modo |
| **05** | Preciso | Whisper → Gemini (áudio e/ou texto — **decisão §5**) | Pipeline 2 etapas | Opcional | — |
| **06** | Ultrapreciso | Whisper → sanitizer → Gemini | Pipeline 3 etapas | Opcional | — |
| **07** | UI modos | Substituir/estender seletor de motores pelos 4 modos | Configurações + copy | Não | Sim (impeccable + copywriting) |
| **08** | Deepgram / dual | Decidir convivência: legado avançado vs esconder vs fallback | Doc + flags | Não | Se aplicável |
| **09** | Hardening | Erros, fallbacks, métricas history, dev mode | Mensagens PT claras | Smoke autorizado | Polish |
| **10** | Verificação | Checklist baseline §8 + builds | Relatório de fase | Smoke se autorizado | — |

**Regra:** não avançar fase sem critério da fase anterior atendido. A Fase 00 **para aqui**.

---

## 5. Decisões de produto ainda abertas

Bloqueiam desenho fino das fases 04–06 (não inventar na implementação):

1. Gemini STT: prompt e modelo exatos; resposta texto puro vs estruturada.
2. Modo Preciso: Gemini recebe **áudio**, **texto Whisper**, ou ambos?
3. Falha parcial (ex.: Whisper ok, Gemini falha): entregar Whisper, erro, ou retry?
4. Dual Deepgram + 4 modos: coexistir na UI, menu “Avançado”, ou só código legado?
5. Sanitizer nos modos ⚡/🚀/🎯: forçado off, opcional, ou só no 💎?
6. Idioma: Deepgram `pt-BR` fixo vs seletor da Transcrição futuramente ligado.

---

## 6. Riscos e mitigações

| Risco | Impacto | Mitigação |
|-------|---------|-----------|
| Big-bang em `audio.rs` | Regressão mic/upload/clipboard | Fase 01: extrair com comportamento idêntico + baseline checklist |
| Misturar Gemini pronúncia e STT | Avaliações quebradas / prompts cruzados | Módulos/funções e prompts distintos |
| Settings antigos incompatíveis | Engine/dual somem após update | `serde(default)` + migração: mapear dual/engine → modo equivalente |
| Sanitizer exige Groq mesmo com STT Gemini | Modo 🚀 sem chave Groq ok; 💎 não | Validar chaves por modo na UI e no Rust |
| Live Deepgram no `start_capture` | Complexidade ao gravar em modos sem Deepgram | Só abrir live se Deepgram no path (já é o caso) |
| Poucos testes automatizados | Regressões silenciosas | Checklist manual baseline; testes unitários do seletor de modo na 01/02 |
| Copy/UI confusa nos 4 modos | Usuário escolhe errado | copywriting: nomes + 1 linha de benefício; impeccable: um seletor claro, não 3 cards legados + 4 modos |
| Chaves no log | Vazamento | Manter logs só com lengths/status; nunca body de key |

---

## 7. Rollback por fase

| Se falhar em… | Ação |
|---------------|------|
| 01 extração | Reverter commits da fase; `audio.rs` monolítico volta |
| 02 contrato | Default settings = paths antigos; feature flag off |
| 03–06 modos | Desligar modo novo; forçar Ultrarrápido/Whisper legado |
| 07 UI | Manter IPC antigo + UI motores atual |
| Runtime user | Reinstalar build baseline; AppData intacto |

Sempre: `git` reset para SHA do checkpoint Fase 00.

---

## 8. Critérios de saída da Fase 00

- [x] Skills `impeccable`, `copywriting`, `tauri` carregadas
- [x] `APP_CONTEXT_HAUMEA_VOICE.md` lido e validado
- [x] `npm run build` OK
- [x] `cargo check` OK
- [x] `cargo test` OK (3/3)
- [x] Baseline documentada (`TRANSCRIPTION_BEHAVIOR_BASELINE.md`)
- [x] Plano de fases/riscos/rollback (`TRANSCRIPTION_MIGRATION_PLAN.md`)
- [x] Nenhuma alteração funcional de app (só docs + este plano)
- [x] Nenhuma chamada de API
- [x] Deepgram não removido
- [ ] Checkpoint git (commit/branch) — executar se o repositório permitir (ver §1)

---

## 9. UX / copy (orientação para fases futuras — não implementar agora)

Princípios (impeccable + copywriting):

- Um controle principal: **modo de transcrição**, não três eixos (engine + dual + deepgram) na superfície default.
- Nomes dos modos já definidos pelo produto; subtítulos curtos focados em **latência vs precisão**, sem jargão de API.
- Deepgram/dual: se permanecerem, área “Avançado” ou equivalente — não competir visualmente com os 4 modos.
- Erros: mensagem acionável (“Configure a chave do Google em Ajustes”) — padrão já usado no backend PT.
- Estados: idle / gravando / processando no gadget **inalterados** em significado.

Sugestão futura (não aplicar na 00): `$impeccable init` para capturar PRODUCT.md/DESIGN.md do app — opcional, não bloqueia a migração.

---

## 10. Ordem de leitura para a Fase 01

1. `docs/TRANSCRIPTION_BEHAVIOR_BASELINE.md`
2. `src-tauri/src/audio.rs` — `stop_capture_inner`, `finalize_transcription`, `transcribe_bytes`
3. `src-tauri/src/models.rs` — `TranscriptionEngine`, `AppState`
4. Este plano §4 Fase 01

**Não iniciar Fase 01 até autorização explícita.**
