# Sonora Context (Chrome/Chromium)

Extensão MV3 mínima para disponibilizar ao aplicativo local somente:

- domínio e URL sem query/fragment;
- título limitado;
- seleção limitada;
- até 800 caracteres próximos ao caret.

Ela não captura a página inteira. Campos `password`, indícios de secrets/tokens e strings de alta entropia são filtrados no content script e novamente no aplicativo.

## Instalação de desenvolvimento

1. Compile o Sonora para obter o executável local.
2. Abra `chrome://extensions`, ative **Modo do desenvolvedor** e use **Carregar sem compactação** nesta pasta.
3. Copie o ID exibido pela extensão.
4. Execute `native-messaging-host/install.ps1 -ExtensionId <id> -ExecutablePath <caminho-do-sonora.exe>`.
5. Recarregue a extensão e habilite as fontes desejadas em Contexto e privacidade no Sonora.

O instalador registra apenas o host Native Messaging no perfil do usuário (`HKCU`). O host entrega os dados ao aplicativo por IPC autenticado em loopback; somente endpoint/token ficam no disco e o contexto bruto permanece em memória. A extensão continua sujeita aos opt-ins do aplicativo e contexto bruto não é persistido no histórico por padrão.
