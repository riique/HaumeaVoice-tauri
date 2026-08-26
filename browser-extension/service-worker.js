const HOST_NAME = "com.haumeavoice.context";
let port = null;

function nativePort() {
  if (port) return port;
  port = chrome.runtime.connectNative(HOST_NAME);
  port.onDisconnect.addListener(() => { port = null; });
  return port;
}

chrome.runtime.onMessage.addListener((message) => {
  if (message?.type !== "haumea-context" || !message.context) return;
  try {
    nativePort().postMessage(message.context);
  } catch {
    port = null;
  }
});
