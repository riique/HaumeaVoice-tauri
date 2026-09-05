# Dados, recuperação e distribuição — 1.0.34

## Recuperação cotidiana

Diagnóstico e recuperação verifica presença do dispositivo, configuração da rota principal e integridade dos arquivos locais. Ele não grava áudio, valida chaves pela rede nem atesta permissões do microfone. A captura aceita até 15 minutos; um WAV incremental permite recuperar frames completos após falha ou cancelamento. Retomar uma transcrição envia áudio aos provedores configurados, somente por ação do usuário.

Cancelar um ditado interrompe o trabalho local. Chamadas já recebidas pelo provedor podem ter sido processadas ou cobradas. Importação, exportação, arquivamento e geração do Voice Profile aguardam conclusão; a interface não oferece cancelamento dessas operações.

Excluir um ditado o remove da lista ativa, mantendo texto e áudio restauráveis. Não há expurgo automático. O arquivamento de áudio é explícito: copia para uma pasta escolhida, verifica o conteúdo, grava o novo vínculo no histórico e só então remove o arquivo original das pastas atuais do aplicativo. Sidecars de áudio original e gravações de recuperação continuam preservados. Mantenha a pasta de arquivo acessível para reproduzir ou retranscrever.

## Backup portátil

Exporte para um novo arquivo JSON. Inclui histórico ativo e removido, notas, vocabulário, snippets, Styles e preferências. Chaves de API nunca entram no backup portátil. Marque a opção de áudio para criar a pasta de mesmo nome com extensão `.media`; mantenha JSON e pasta juntos. Áudios ainda sem entrada no histórico permanecem na recuperação local e devem ser preservados em um backup completo do AppData.

Na importação, IDs existentes são preservados. Notas, snippets, vocabulário e Styles são mesclados; destino, privacidade, provedor e atalhos ativos continuam os desta instalação. Áudios são copiados para uma nova pasta local e vinculados às respectivas entradas. A importação valida todos os metadados e caminhos de mídia antes de gravar. O limite dos metadados é 256 MiB; cada mídia aceita até 350 MiB.

Antes da importação, `before-import-<timestamp>/` preserva os arquivos originais. Uma falha entre arquivos pode exigir restauração desse checkpoint: encerre o aplicativo, preserve o estado atual, copie os arquivos do checkpoint para o AppData e só então remova `import-in-progress.json`. Não misture arquivos de checkpoints diferentes. A última transação JSONL incompleta pode ser reparada pela interface; o conteúdo original fica em `.bak`. Corrupção anterior exige restauração de backup.

As chaves existentes migram para DPAPI da conta Windows após verificação criptográfica. A cópia `.migration.dpapi` é criptografada. DPAPI não é portátil para outra conta/máquina e não protege contra código já executado com a autoridade do próprio usuário. Um antigo `browser-context.json`, se presente, vira uma cópia DPAPI verificada antes da remoção do arquivo em claro.

## Benchmark offline

`npm run benchmark -- docs/benchmark-fixture.jsonl <novo-arquivo-de-resultados.json>` valida o harness com texto sintético. Não chama APIs. Para avaliar modelos, forneça referências revisadas e transcrições obtidas com autorização independente, separadas por `pipeline`.

Cada linha contém `reference`, `hypothesis`, termos literais opcionais e, quando medidos, `latency_ms`, `audio_seconds` e `cost_usd`. WER/CER normalizam NFC, caixa e pontuação; a preservação de termos compara a grafia literal, adequada a nomes e código. Segmente textos com mais de 10 mil caracteres. Ausência de custo/duração produz `null`, nunca custo zero estimado. O fixture não demonstra qualidade relativa dos modos ou provedores. Use amostras de português, code-switching, nomes, código, silêncio e ruído com consentimento.

## Qualificação e distribuição

Ambiente qualificado: Windows x64, Node 24 e Rust 1.97.1. `npm run lint` executa TypeScript estrito e contratos de versões, CSP e capabilities. A CI também roda testes, clippy sem warnings, scanners e Tauri build. O workflow usa ações fixadas por commit, permissão de leitura e apenas artefatos da execução; não publica release automaticamente.

Os installers e `SHA256SUMS.txt` permitem conferir integridade. Um checksum sozinho não atesta a identidade do editor. Authenticode requer certificado de distribuição do proprietário, ausente nesta conta na qualificação de 2026-09-04. Não foi criada assinatura fictícia. Para distribuição pública, assine EXE/MSI/NSIS pelo processo de custódia do certificado e verifique a cadeia e o timestamp antes de publicar.

Upgrade local: preserve o instalador anterior e um backup completo do AppData, encerre o processo, execute NSIS `/S`, confira exit code, versão do executável e registro, e reabra. Rollback para 1.0.33 exige também restaurar o checkpoint anterior do AppData: essa versão não entende o journal incremental nem as credenciais DPAPI. Preserve uma cópia dos dados novos antes de retornar. Não reinstale a versão anterior sobre dados novos esperando compatibilidade.

Os testes automatizados não substituem validação física de microfone/hotplug/mute, atalhos, UI Automation, clipboard, múltiplos monitores, extensão carregada no navegador, APIs reais e estabilidade prolongada. Não foi usado computer use nesta implementação. A extensão deve ser recarregada pelo usuário para aplicar o protocolo novo.
