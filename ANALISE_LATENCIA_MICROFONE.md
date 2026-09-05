# Análise de Latência de Inicialização do Microfone e Soluções para Cortar Início de Fala

## 1. Diagnóstico do Problema

Durante o uso do Sonora, observa-se frequentemente um pequeno atraso (*delay*) entre o acionamento da *keybind* de transcrição e o momento em que o microfone efetivamente começa a gravar o áudio. Isso faz com que as primeiras sílabas ou palavras ditas pelo usuário sejam cortadas (*chopped audio*).

### Causa Raiz Técnica no Sonora

Conforme inspecionado em [`src-tauri/src/audio.rs`](<C:/Dev/Projects/HaumeaVoice-tauri/src-tauri/src/audio.rs>), a gravação é disparada pela rotina `audio::start_capture()`, que executa o seguinte fluxo síncrono após o pressionamento do atalho global (`shortcuts.rs`):

1. **Leitura de Configuração de Disco**: Chama `crate::settings::load_input_device()` lendo arquivo local em I/O de disco.
2. **Enumeração de Dispositivos WASAPI**: Interroga a host CPAL (`host.input_devices()`) e busca o nome do dispositivo na lista do Windows.
3. **Consulta de Capacidades do Hardware**: Consulta a configuração padrão suportada (`device.default_input_config()`).
4. **Instanciação e Play da Stream CPAL**: Cria a stream com `device.build_input_stream()` e solicita o início com `stream.play()`.
5. **Cold Start do Driver WASAPI (Windows)**: O subsistema de áudio do Windows leva de **100 ms a 500 ms** para realizar a transição do estado inativo (*stopped*) para capturando (*running*) no driver da placa de som.

Todo esse processo consome entre 150 ms e 600 ms antes que a primeira amostra PCM chegue ao callback do Rust.

---

## 2. Solução A: Microfone Sempre Ativo (*Always Warm Stream*) com Ring Buffer

### Como Funciona
O aplicativo inicializa a captura CPAL durante a inicialização (`app_setup`) e a mantém executando continuamente em segundo plano.

- As amostras capturadas alimentam um **Ring Buffer (Buffer Circular)** de 500 ms (ex: ~16.000 amostras em 16 kHz mono).
- Quando a gravação não está ativa (`is_recording == false`), as amostras antigas são descartadas continuamente do Ring Buffer.
- Ao pressionar a keybind, a flag `is_recording` torna-se `true`, o conteúdo acumulado do Ring Buffer é injetado como *pre-roll* no buffer de transcrição, e as novas amostras continuam fluindo sem interrupção.

### Vantagens
- **Zero Latência de Hardware**: Gravação 100% instantânea sem tempo de resposta do driver de som.
- **Captura do Passado Recente**: Palavras ditas frações de segundo *antes* do atalho ser pressionado são capturadas através do *pre-roll*.

### Desvantagens e Riscos
1. **Ícone de Privacidade no Windows 11**: O ícone do microfone na Barra de Tarefas permanecerá continuamente visível.
2. **Fones de Ouvido Bluetooth (HFP vs A2DP)**: Dispositivos Bluetooth (como fones e headsets) são forçados pelo Windows a alternar do modo estéreo de alta qualidade (A2DP) para o modo mono/chamada (HFP), prejudicando o áudio do sistema enquanto o app estiver aberto.
3. **Consumo de Bateria/CPU**: O callback da placa de som continuará executando dezenas de vezes por segundo, impedindo modos profundos de economia de energia da placa de som.

---

## 3. Soluções Alternativas (Sem Microfone Aberto 24/7)

### Solução B: Cache de Dispositivo e Pré-configuração CPAL em RAM (Zero-I/O Startup)
- **Como funciona**: Manter o `cpal::Device` pré-resolvido e a sua `StreamConfig` armazenados no `AppState` durante a inicialização do app ou ao trocar de dispositivo nas configurações.
- **Ganho**: Elimina os ~50–150 ms gastos em I/O de disco (`load_input_device`) e enumeração de dispositivos no Windows na hora da gravação.

### Solução C: *Warm-up* Temporário por Sessão (Timer de Inatividade)
- **Como funciona**: O primeiro acionamento abre o microfone. Ao finalizar a transcrição, a stream permanece aberta em segundo plano com um timer de inatividade (ex: 3 a 5 minutos). Se novo atalho for pressionado durante esse tempo, o início é instantâneo. Após o tempo limite sem uso, a stream é fechada.
- **Ganho**: Proporciona latência zero para sessões ativas de trabalho, reduzindo o impacto de bateria e o ícone de privacidade durante longos períodos inativos.

### Solução D: *KeyDown Warmup* (Pré-aquecimento no Pressionar de Teclas)
- **Como funciona**: Disparar o `build_input_stream()` / `stream.play()` no evento de *KeyDown* da primeira tecla modificadora do atalho (ex: ao pressionar a tecla `Ctrl`), antes de o atalho completo (`Ctrl+B`) ser solto ou acionado por completo.
- **Ganho**: Ganha de 100 ms a 200 ms da movimentação física da mão do usuário.

### Solução E: Feedback de Pronto Orientado por Hardware (*Ready Cue*)
- **Como funciona**: Atualizar o estado visual do gadget ou emitir um curto sinal sonoro somente *após* a recepção dos primeiros pacotes de áudio válidos no callback de áudio.
- **Ganho**: Reduz o erro humano de fala antecipada através de sincronização de UX.

---

## 4. Matriz Comparativa de Soluções

| Estratégia | Redução de Latência | Impacto em Bateria / Ícone Win11 | Compatibilidade Bluetooth | Complexidade |
| :--- | :--- | :--- | :--- | :--- |
| **A. Microfone Sempre Ativo (Warm Stream)** | 100% (Instantâneo + Pre-roll) | Alto (Ícone visível 24/7) | Baixa (Força modo HFP) | Média |
| **B. Cache de Dispositivo em RAM** | ~50 ms a 150 ms | Nenhum | Alta | Baixa |
| **C. Warm-up Temporário (Timer de 3-5 min)** | 100% durante uso ativo | Médio (Apenas durante a sessão) | Média | Média |
| **D. KeyDown Warmup** | ~100 ms a 200 ms | Nenhum | Alta | Média |
| **E. Feedback de Pronto no Gadget** | Contorna via UX | Nenhum | Alta | Baixa |

---

## 5. Recomendação de Arquitetura Proposta para o Sonora

Para entregar a melhor experiência sem comprometer usuários de fones Bluetooth e notebooks, recomenda-se a seguinte abordagem combinada:

1. **Implementar Cache de Dispositivo (Solução B)** no `AppState` para eliminar o overhead de busca e I/O ao apertar o atalho.
2. **Implementar a Opção Configurável "Modo Latência Zero / Microfone Pré-Aquecido" (Solução A + C)**:
   - Adicionar chave em [`src-tauri/src/settings.rs`](<C:/Dev/Projects/HaumeaVoice-tauri/src-tauri/src/settings.rs>) e toggle em [`src/views/ConfiguracoesView.tsx`](<C:/Dev/Projects/HaumeaVoice-tauri/src/views/ConfiguracoesView.tsx>).
   - Permitir ao usuário escolher entre:
     - **Padrão (Econômico / Compatível)**: Inicialização rápida com cache + *Warm-up* por sessão de 3 minutos.
     - **Latência Zero (Sempre Ativo com Ring Buffer)**: Mantém o microfone aberto continuamente com buffer pre-roll de 500 ms.
