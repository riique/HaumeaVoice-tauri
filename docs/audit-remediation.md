# Auditoria implementada — Haumea Voice 1.0.34

Base: `e99b7a33513371a4b961ef7b4662989c4fa1cc07`, versão 1.0.33. Implementação local em `codex/audit-remediation-1.0.34`, solicitada em 2026-09-04. Sem commit, push ou publicação remota.

## Cobertura dos achados

| Achado | Implementação e evidência | Limite da qualificação |
|---|---|---|
| F01 — segredos em URLs/erros | Gemini usa x-goog-api-key; transporte remove URL; redação central e teste com segredo sintético. | Sem chamadas reais a provedores. |
| F02 — credenciais em claro/IPC | DPAPI CurrentUser com migração verificada; renderer recebe referências opacas; disco precede confirmação. Credenciais instaladas equivalentes ao checkpoint. | DPAPI depende da conta Windows. |
| F03 — integridade | Escrita atômica com backup, rejeição de JSON inválido, journal durável, erros de salvamento visíveis e remoção reversível. | Importação de vários arquivos usa checkpoint e marcador; interrupção pode exigir restauração manual. |
| F04 — concorrência | RMW sob lock, disco antes da publicação em memória, snapshot da pipeline e fila sequencial de autosave. Teste de alterações concorrentes e formato inválido. | Sem estresse prolongado com interação humana. |
| F05 — mute | Removida a chamada que desmutava todos os endpoints; dispositivo configurado ausente produz erro. | Mute/hotplug físicos pendentes. |
| F06 — operações sobrepostas | Coordenador exclusivo para captura, upload, retry, teste de mic, pronúncia e tarefas auxiliares; eventos vinculados à operação. | Sem APIs reais concorrentes. |
| F07 — destino de paste | Identidade UI Automation do campo, HWND/PID e cancelamento conferidos junto da injeção. | Interoperabilidade entre controles Windows/monitores precisa de validação física. |
| F08 — coleta | Extensão coleta somente as fontes permitidas em solicitação vigente. Código real exercitado em VM JavaScript. | Recarregar a extensão instalada manualmente. |
| F09 — contexto obsoleto | Nonce, prazo curto, documento/janela/aba ativa, conferência posterior e consumo único. | Sem automação do navegador real. |
| F10 — sucesso incorreto | Falhas de entrega/persistência não confirmam salvamento. Texto continua disponível; teste da projeção de erro com texto preservado. | Clipboard/paste reais não exercitados. |
| F11 — captura | Fila e buffer limitados, WAV incremental, limite de 15 minutos, falha de stream encerra captura e mantém áudio. | Sem queda de energia/dispositivo real. |
| F12 — cancelamento | Ditados, uploads e retries canceláveis; permissões de entrega expiram com cancelamento/término. | Manutenção e Voice Profile aguardam conclusão; cancelamento local não desfaz cobrança remota. |
| F13 — histórico | Journal incremental, índice em memória, páginas de 50, busca no backend, detalhes sob demanda, restauração e arquivamento explícito. | Memória ainda cresce com o conteúdo; não medido cold start com 100 mil runs reais. |
| F14 — Scratchpad | Eventos, loading, erro/retry, feedback de clipboard e confirmação de exclusão. | Testes DOM com bridge simulada. |
| F15 — formato/rota | Meta rejeita não-WAV antes da requisição; Início identifica rota/provedor/modelo. | Sem medição de reconhecimento da Meta. |
| F16 — acessibilidade | Labels, menu com teclado/foco, erros visíveis, bloqueio durante operação e janela ajustada à área útil. | Revisão estática/DOM; sem inspeção visual/leitor de tela. |
| F17 — dependências | Atualizações direcionadas: Vite 6.4.3, h2 0.4.16, plist 1.10.0/quick-xml 0.41, anyhow 1.0.104 e memmap2 0.9.11. | Permanecem avisos informativos RustSec, triados abaixo. |
| F18 — qualidade | TypeScript estrito, contratos de versão/CSP/75 comandos, testes React/privacidade/Rust e CI Windows com clippy sem warnings. | Workflow criado, sem execução remota. |
| F19 — distribuição | Instância única, logs rotativos/redigidos, capabilities separadas, release sem devtools, NSIS/MSI e SHA-256. | Authenticode pendente de certificado do proprietário. |

## Melhorias de produto

Diagnóstico local pelo Início; recuperação de áudios interrompidos e itens removidos; backup portátil com mídia opcional; arquivamento sem limpeza automática; seleção rápida de destino/Style; benchmark offline com WER/CER, termos, latência e custo fornecidos pelo usuário; filtro anti-alias; rejeição conservadora de silêncio digital; confidence identificada como não calibrada em Insights.

O benchmark não chama modelos nem demonstra ordenação de qualidade entre os modos. O filtro atenuou o tom de 12 kHz em pelo menos 40 dB na conversão 48→16 kHz, preservando o tom de 1 kHz. Não foi medida melhora de WER em voz real.

## Verificação de 2026-09-04

- 210 testes Rust, 18 testes frontend e 6 testes React aprovados.
- Formatação Rust, clippy sem warnings, TypeScript/contratos, frontend build e Tauri release aprovados.
- Fixture com 100 mil entradas: página de 50 em 96 ms, payload 50.067 bytes, pico do processo de teste de 214,34 MiB. Orçamentos automatizados: página abaixo de 5 s e payload abaixo de 256 KB. Medição do cache/projeção em build debug, sem chamadas a APIs.
- Um minuto de PCM 48 kHz convertido para 16 kHz em aproximadamente 2,1 s no build debug, dentro do orçamento de 30 s.
- npm audit: zero vulnerabilidades. cargo audit: zero entradas de vulnerabilidade e 17 avisos informativos. Doze pacotes GTK/glib/proc-macro-error não alcançam Windows; cinco UNIC sem manutenção permanecem via urlpattern/tauri-utils. O aviso unsound de glib está entre os excluídos do target Windows. Nenhum aviso foi silenciado.
- NSIS: exit code 0. Executável/registro: 1.0.34. Processo reaberto, setup completo e sem linhas ERROR no smoke. Segunda abertura: exit code 0, uma instância.
- AppData: 105 arquivos anteriores, 108 após upgrade; nenhum ausente, 101 idênticos por SHA-256. Quatro alterações esperadas: credenciais DPAPI, endpoint efêmero, cache Insights e log. Credenciais, Voice Profile e consentimento comparados e preservados.
- Backups DPAPI completos e instalador anterior em `F:\Dev-Backup\HaumeaVoice\2026-09-04-1.0.34`. Rollback documentado; nenhum downgrade executado.

Evidências: [pasta de qualificação](C:/Users/Henrique/.codex/visualizations/2026/09/04/01a06ebd-005b-75e2-a480-ea7b3d495324/implementation-1.0.34). Procedimentos: [dados e distribuição](recovery-and-release.md).

Não houve computer use, chamadas a provedores de IA, subagentes, commit, push ou deploy. Serena apoiou a análise inicial; Obsidian registra as decisões duráveis. Continuam pendentes validação física/UI/API e assinatura confiável, sem simular resultados dessas etapas.
