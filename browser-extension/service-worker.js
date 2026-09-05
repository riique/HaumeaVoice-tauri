const HOST_NAME = "com.haumeavoice.context";
let port;
let inFlight;
function connect() {
  if (port) return;
  try {
    port = chrome.runtime.connectNative(HOST_NAME);
    port.onDisconnect.addListener(() => { port = null; inFlight = null; });
    port.onMessage.addListener(async (message) => {
      const request = message?.request;
      if (!request || inFlight === request.request_id) return;
      inFlight = request.request_id;
      try {
        const window = await chrome.windows.getLastFocused();
        if (!window.focused) return;
        const [tab] = await chrome.tabs.query({ active: true, windowId: window.id });
        if (!tab?.id) return;
        const result = await chrome.tabs.sendMessage(tab.id, { type: "haumea-capture", request });
        const afterWindow = await chrome.windows.getLastFocused();
        const [afterTab] = await chrome.tabs.query({ active: true, windowId: window.id });
        if (!afterWindow.focused || afterWindow.id !== window.id || afterTab?.id !== tab.id || !result?.document_focused || Date.now() > request.expires_at_ms) return;
        port?.postMessage({ kind: "context", ...result, tab_id: tab.id, window_id: window.id });
      } catch { /* Missing permission or receiver: native context remains unavailable. */ }
    });
  } catch { port = null; }
}
// Polling contains no page data. Native messaging keeps this worker alive.
setInterval(() => {
  connect();
  try { port?.postMessage({ kind: "poll" }); } catch { port = null; }
}, 150);
connect();
