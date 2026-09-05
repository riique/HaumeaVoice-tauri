let MAX_CONTEXT_CHARS = 800;

function sensitiveElement(element) {
  if (!(element instanceof HTMLElement)) return false;
  if (element instanceof HTMLInputElement && element.type.toLowerCase() === "password") return true;
  const hint = [element.getAttribute("autocomplete"), element.getAttribute("name"), element.getAttribute("id")]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
  return /(password|passwd|secret|api.?key|access.?token|credit.?card|one.?time.?code)/.test(hint);
}

function looksSensitive(text) {
  if (!text) return false;
  if (/(password|passwd|authorization|api[_-]?key)\s*[:=]/i.test(text)) return true;
  return text.split(/\s+/).some((token) => {
    const cleaned = token.replace(/^[^A-Za-z0-9]+|[^A-Za-z0-9._+-]+$/g, "");
    if (/^(sk-|gsk_|ghp_|AIza|xoxb-)/.test(cleaned)) return true;
    if (cleaned.length > 40 && cleaned.split(".").length === 3) return true;
    const classes = [/[a-z]/, /[A-Z]/, /\d/, /[-_+/=]/].filter((pattern) => pattern.test(cleaned)).length;
    return cleaned.length >= 40 && classes >= 3;
  });
}

function limited(text) {
  const normalized = String(text || "").trim();
  if (!normalized || looksSensitive(normalized)) return null;
  return normalized.slice(0, MAX_CONTEXT_CHARS);
}

function selectionText() {
  const active = document.activeElement;
  if (sensitiveElement(active)) return null;
  if (active instanceof HTMLInputElement || active instanceof HTMLTextAreaElement) {
    const start = active.selectionStart ?? 0;
    const end = active.selectionEnd ?? start;
    return limited(active.value.slice(start, end));
  }
  return limited(window.getSelection()?.toString());
}

function nearbyText() {
  const active = document.activeElement;
  if (sensitiveElement(active)) return null;
  if (active instanceof HTMLInputElement || active instanceof HTMLTextAreaElement) {
    const caret = active.selectionStart ?? 0;
    const half = Math.floor(MAX_CONTEXT_CHARS / 2);
    return limited(active.value.slice(Math.max(0, caret - half), caret + half));
  }
  const selection = window.getSelection();
  if (!selection?.rangeCount) return null;
  const range = selection.getRangeAt(0);
  const node = range.startContainer;
  const text = node.textContent || "";
  const caret = node.nodeType === Node.TEXT_NODE ? range.startOffset : 0;
  const half = Math.floor(MAX_CONTEXT_CHARS / 2);
  return limited(text.slice(Math.max(0, caret - half), caret + half));
}

// Collection happens only in response to a short-lived native request.
chrome.runtime.onMessage.addListener((message, sender, respond) => {
  if (sender.id !== chrome.runtime.id || message?.type !== "sonora-capture") return;
  const request = message.request;
  if (!request?.request_id || Date.now() > request.expires_at_ms || !document.hasFocus() || document.visibilityState !== "visible") return;
  if (!/^https?:$/.test(location.protocol)) return;
  MAX_CONTEXT_CHARS = Math.min(4000, Math.max(100, request.max_chars || 800));
  const context = {
    domain: location.hostname.toLowerCase(), url: null,
    title: request.title ? limited(document.title)?.slice(0, 200) || null : null,
    selection: request.selection ? selectionText() : null,
    nearby_text: request.nearby_text ? nearbyText() : null,
    captured_at_ms: Date.now()
  };
  respond({ request_id: request.request_id, document_focused: document.hasFocus(), context });
});
