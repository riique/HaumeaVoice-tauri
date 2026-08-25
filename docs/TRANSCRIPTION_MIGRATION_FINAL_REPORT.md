# Relatório final — Migração de pipelines de transcrição

**Produto:** Haumea Voice 1.0.3  
**Escopo:** Fases 00–09  
**Data:** 2026-07-18  

---

## Resumo executivo

A orquestração de transcrição saiu de um monólito em `audio.rs` para:

1. **Contratos** (`pipeline_contract`, `vocabulary`, `sanitizer_json`)
2. **Módulo `transcription/`** (legado + modos de produto)
3. **Módulo `gemini/`** (Files API, STT, refine, pronúncia)
4. **UI centrada em pipelines** (Configurações redesenhada)
5. **Histórico observável** (modo, modelos, intermediários, fallback, ações)

Deepgram permanece **experimental** no legado avançado. Chaves continuam em plain text no AppData (Credential Manager documentado como melhoria futura).

---

## Pipelines de produto

| Modo | Fluxo | Sanitizer | Fallback principal |
|------|--------|-----------|-------------------|
| ⚡ Ultrarrápido | Whisper | Não | — |
| 🚀 Rápido e preciso | Gemini Files | Não | Whisper (configurável) |
| 🎯 Preciso | Whisper ∥ upload → refine | Não | Whisper / Gemini puro |
| 💎 Ultrapreciso | Whisper ∥ upload → sanitizer → Gemini | Sim (JSON) | Sanitizado / Whisper |

**Tipo de conteúdo:** Automático | Programação | Estudo

**Legado:** motor manual + dual Whisper/Deepgram + validador — disponível com “modos” desligados.

---

## Auditoria

| Recurso | Estado | Evidência | Risco restante |
|---------|--------|-----------|----------------|
| Gravação mic + clipboard + paste | OK | `audio.rs`, atalhos | Drivers WASAPI / foco |
| Upload arquivo | OK | `transcribe_file_path` | Arquivos >50 MB rejeitados |
| Ultrarrápido | OK | `modes::run_ultra_fast` + OpenRouter STT, Whisper/Groq fixo | Depende chave OpenRouter e disponibilidade Groq |
| Rápido e preciso | OK | `run_fast_accurate` + Files API | Cota/modelo Gemini |
| Preciso | OK | `run_precise` paralelo | Latência = max(whisper,upload)+refine |
| Ultrapreciso | OK | `run_ultra_precise` | Mais etapas = mais pontos de falha |
| Sanitizer JSON | OK | `sanitizer_json` + prompt versionado | Modelo pode ainda falhar formato → raw |
| Vocabulário estruturado | OK | `vocabulary.rs` + UI | Strict só aliases inequívocos |
| Deepgram | Experimental | Avançado legado | Não é padrão de produto |
| Dual engine | Legado | Avançado; não misturado com cards | Confusão se usuário reativar legado |
| Histórico (CRUD parcial) | OK | copiar/editar/excluir/detalhes/retranscrever | Sem busca full-text avançada |
| Pronúncia | OK | `gemini::evaluate_pronunciation` separada | Independente dos modos STT |
| Gadget + atalhos + tray | OK | inalterados no fluxo | AppHang histórico mitigado, monitorar |
| Timestamp local | OK | `GetLocalTime` Windows | Não-Windows usa UTC civil |
| Chaves API | Parcial | `api_keys.json` plain text | Credential Manager não migrado |
| Testes automatizados | OK | `cargo test` (76+) | Sem E2E com APIs reais |
| Release | Não feito | — | Build manual necessário |

---

## Teste manual guiado

1. **Gravação** — Ctrl+B (ou atalho configurado): grava → para → texto no campo focado + card no Histórico.  
2. **Cancel** — inicia gravação, Ctrl+Q: sem texto novo.  
3. **Ultrarrápido** — Pipelines → selecionar → gravar curto.  
4. **Rápido e preciso** — chave Google; testar com e sem fallback Whisper.  
5. **Preciso** — verificar selo de modo e latência; forçar falha de rede se possível.  
6. **Ultrapreciso** — Groq+Google; conferir estágios nos Detalhes.  
7. **Upload** — Transcrição → arquivo WAV/MP3.  
8. **Clipboard** — mic cola; upload **não** cola.  
9. **Histórico** — copiar, editar, excluir, detalhes, retranscrever, avaliar pronúncia.  
10. **Vocabulário** — termo strict + alias multi-palavra; retranscrever.  
11. **Gadget** — idle/gravando/processando; arrastar; compacto.  
12. **Atalhos** — rebind e validar.  
13. **Bandeja** — fechar janela → app na bandeja; Sair encerra.  
14. **Legado** — Pipelines → Avançado → desligar modos → dual/Deepgram.

**Não rodar release automática.** Build sugerido: `npm run tauri build`.

---

## Arquivos-chave

```text
src-tauri/src/transcription/   # legado + modes
src-tauri/src/gemini/            # Files API + STT/refine/pronúncia
src-tauri/src/vocabulary.rs
src-tauri/src/sanitizer_json.rs
src-tauri/src/audio.rs           # captura + I/O
src/views/ConfiguracoesView.tsx  # UI pipelines
src/views/HistoricoView.tsx
docs/TRANSCRIPTION_MIGRATION_PLAN.md
docs/TRANSCRIPTION_BEHAVIOR_BASELINE.md
APP_CONTEXT_HAUMEA_VOICE.md
```

---

## O que não foi feito de propósito

- Release / assinatura de código  
- Windows Credential Manager (risco de migração de chaves)  
- Remoção do código legado Deepgram/dual  
- Testes E2E com APIs reais  
- OpenRouter  

---

## Conclusão

A migração de arquitetura está **pronta para revisão humana e build de release manual**. Validar o checklist acima com chaves reais antes de distribuir.
