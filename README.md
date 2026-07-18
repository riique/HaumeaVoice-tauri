# Haumea Voice

Aplicativo desktop de **digitação por voz** para Windows. Grave com um atalho global, o app transcreve com motores em nuvem, opcionalmente refina o texto e cola no campo focado (`Ctrl+V`).

**Versão:** 1.0.3 · **Stack:** Tauri 2 · React 18 · TypeScript · Rust

---

## O que faz

- Gravação pelo microfone (atalho global ou botão na UI)
- Transcrição em nuvem com **pipelines de produto** (Groq Whisper + Google Gemini)
- Cola automática no campo focado (clipboard + simulação de paste)
- Upload de arquivos de áudio (WAV, MP3, etc.)
- Histórico local com detalhes de pipeline, retranscrição e avaliação de pronúncia
- Vocabulário estruturado (termos, aliases, categorias, literais strict)
- Overlay **gadget** sempre no topo + bandeja do sistema
- Atalhos globais configuráveis (padrão: `Ctrl+B` grava, `Ctrl+Q` cancela)

---

## Novidades — pipelines de transcrição

A orquestração saiu de um monólito em `audio.rs` para módulos dedicados (`transcription/`, `gemini/`, contratos e vocabulário). A UI de **Configurações** passa a ser centrada em **pipelines de produto**, com o modo legado disponível em *Avançado*.

### Modos de produto

| Modo | Fluxo | Sanitizer | Fallback típico |
|------|--------|-----------|-----------------|
| ⚡ **Ultrarrápido** | Groq Whisper | Não | — |
| 🚀 **Rápido e preciso** | Gemini (Files API / inline) | Não | Whisper (configurável) |
| 🎯 **Preciso** | Whisper ∥ upload → refine Gemini | Não | Whisper ou Gemini puro |
| 💎 **Ultrapreciso** | Whisper ∥ upload → sanitizer → Gemini | Sim (JSON) | Texto sanitizado / Whisper |

### Tipo de conteúdo

Hint opcional para o prompt de refinamento:

- **Automático**
- **Programação**
- **Texto comum**
- **Estudo**

### Outras melhorias desta entrega

- **Módulo Gemini** — Files API, STT, refine (preciso/ultrapreciso), transporte e avaliação de pronúncia
- **Sanitizer JSON** — saída estruturada no modo Ultrapreciso, com fallback para texto bruto se o formato falhar
- **Vocabulário estruturado** — canônico + aliases + categoria + flag *strict* (substitui a lista simples de palavras)
- **Histórico observável** — modo, modelos, estágios, textos intermediários, fallback, latências; copiar / editar / excluir / detalhes / retranscrever / pronúncia
- **UI de Configurações** — cards de pipeline, tipo de conteúdo e painel avançado (legado)
- **Deepgram** — permanece **experimental** no caminho legado (motor manual / dual Whisper+Deepgram)
- **Telemetria local** — latência por estágio, RTF estimado, throughput (sem analytics externo)
- Testes unitários Rust no pipeline (`cargo test`)

### Arquitetura (visão rápida)

```text
src-tauri/src/
├── audio.rs                 # captura mic, WAV, clipboard/paste
├── transcription/           # legado + modos de produto + telemetria
├── gemini/                  # Files API, STT, refine, pronúncia
├── pipeline_contract.rs     # TranscriptionMode, ContentType, estágios
├── vocabulary.rs            # termos estruturados
├── sanitizer_json.rs        # parse da saída JSON do sanitizer
├── groq.rs / deepgram.rs    # Whisper e Deepgram (legado/fallback)
└── history.rs / settings.rs

src/views/
├── ConfiguracoesView.tsx    # pipelines + avançado + vocabulário
├── HistoricoView.tsx        # cards, detalhes, ações
└── TranscricaoView.tsx      # upload de arquivo
```

Documentação técnica da migração: [`docs/TRANSCRIPTION_MIGRATION_FINAL_REPORT.md`](docs/TRANSCRIPTION_MIGRATION_FINAL_REPORT.md).

---

## Requisitos

1. **Node.js** (npm)
2. **Rust** via [rustup](https://rustup.rs/)
3. Chaves de API (conforme o modo):
   - **Groq** — Ultrarrápido, Preciso, Ultrapreciso e fallbacks
   - **Google (Gemini)** — Rápido e preciso, Preciso, Ultrapreciso, pronúncia
   - **Deepgram** — apenas no modo legado avançado

---

## Desenvolvimento

```bash
npm install
npm run tauri dev
```

Só a UI (sem backend nativo completo):

```bash
npm run dev
```

---

## Build de produção

```bash
npm run tauri build
```

Se o `cargo` não estiver no `PATH` (Windows / PowerShell):

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH
npm run tauri build
```

**Artefatos típicos:**

- Executável: `src-tauri/target/release/haumea-voice.exe`
- Instaladores: `src-tauri/target/release/bundle/nsis/` e `bundle/msi/`

Detalhes: [`BUILD.md`](BUILD.md).

---

## Uso rápido

1. Abra **Configurações** e salve as chaves de API necessárias.
2. Escolha um **pipeline** (Ultrarrápido, Rápido e preciso, Preciso ou Ultrapreciso).
3. Foque o campo de texto de destino e use o atalho de gravação (`Ctrl+B` por padrão).
4. Pare a gravação com o mesmo atalho; o texto final é colado no campo focado e entra no **Histórico**.
5. Cancele com `Ctrl+Q` (sem gerar texto novo).

**Notas:**

- Gravação pelo mic **cola** no campo focado; upload de arquivo **não** cola automaticamente.
- Arquivos de áudio grandes (> ~50 MB) são rejeitados.
- Dados locais (histórico, settings, chaves, áudios): `%APPDATA%\com.haumeavoice.app\`

---

## Telas

| View | Função |
|------|--------|
| Início | Status, contadores, iniciar gravação |
| Transcrição | Upload de arquivo |
| Histórico | Entradas, métricas, retranscrever, pronúncia |
| Atalhos | Rebind de toggle/cancel |
| Configurações | Pipelines, chaves, vocabulário, avançado |
| Gadget | Overlay compacto always-on-top |

---

## Stack

| Camada | Tecnologia |
|--------|------------|
| Shell | Tauri 2 |
| Frontend | React 18, TypeScript, Vite 5, Tailwind CSS 3 |
| Backend | Rust (captura, STT, IPC, OS) |
| Áudio | cpal (WASAPI no Windows) |
| STT / LLM | Groq Whisper, Gemini, Deepgram (legado) |
| Clipboard / paste | arboard + enigo |

---

## Testes

```bash
cd src-tauri
cargo test
```

Checklist manual: [`docs/MANUAL_TEST_CHECKLIST.md`](docs/MANUAL_TEST_CHECKLIST.md).

---

## Licença

Consulte o repositório para a licença aplicável. Chaves de API e dados de uso são de responsabilidade do usuário; as chaves são armazenadas em texto no AppData da aplicação (migração para Credential Manager é melhoria futura).
