# Checklist de teste manual — Haumea Voice (pós-migração)

Marque cada item após validar em build local (`cargo tauri dev` ou exe de debug).

## Preparação

- [ ] Chave Groq configurada (Provedores e APIs)
- [ ] Chave Google configurada
- [ ] (Opcional) Deepgram para legado
- [ ] Microfone testado em Geral

## Gravação e atalhos

- [ ] Iniciar/parar gravação pelo botão Início
- [ ] Atalho toggle (padrão Ctrl+B)
- [ ] Atalho cancel (padrão Ctrl+Q) descarta sem histórico de sucesso
- [ ] Texto colado no campo focado (mic)
- [ ] Gadget: idle → gravando → processando → idle

## Quatro modos

- [ ] Ultrarrápido produz texto com chave Groq
- [ ] Rápido e preciso com Google
- [ ] Rápido e preciso: desligar fallback e ver erro se Gemini falhar
- [ ] Preciso completa e mostra modo no Histórico
- [ ] Ultrapreciso completa (Groq + Google)

## Upload

- [ ] Arquivo WAV/MP3 em Transcrição
- [ ] Upload **não** sobrescreve clipboard
- [ ] Entrada aparece no Histórico com source file

## Histórico

- [ ] Copiar texto
- [ ] Editar e salvar
- [ ] Detalhes técnicos (recolhidos por padrão)
- [ ] Retranscrever
- [ ] Excluir item
- [ ] Avaliar pronúncia (separado do pipeline STT)
- [ ] Entrada antiga (pré-migração) ainda abre

## Vocabulário

- [ ] Adicionar termo + alias multi-palavra + Literal
- [ ] Buscar / editar / remover
- [ ] Retranscrever e verificar preservação de literal

## Sistema

- [ ] Fechar janela → bandeja
- [ ] Sair pelo menu da bandeja
- [ ] Autostart (opcional)
- [ ] Logs em `%APPDATA%\com.haumeavoice.app\logs\`

## Não bloquear release se

- Deepgram experimental falhar (não é caminho padrão)
- Reasoning GPT-OSS desligado (padrão)

## Build de release (manual)

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH
npm run tauri build
```
