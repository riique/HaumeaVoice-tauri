const MAX_CONTEXT_CHARS = 800;
let pendingTimer = 0;

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

function snapshot() {
  const domain = location.hostname.toLowerCase();
  if (!domain || !/^https?:$/.test(location.protocol)) return null;
  return {
    domain,
    url: `${location.origin}${location.pathname}`.slice(0, 500),
    title: limited(document.title)?.slice(0, 200) || null,
    selection: selectionText(),
    nearby_text: nearbyText(),
    captured_at_ms: Date.now()
  };
}

function publish() {
  window.clearTimeout(pendingTimer);
  pendingTimer = window.setTimeout(() => {
    const context = snapshot();
    if (context) chrome.runtime.sendMessage({ type: "haumea-context", context }).catch(() => undefined);
  }, 180);
}

document.addEventListener("selectionchange", publish, { passive: true });
document.addEventListener("focusin", publish, { passive: true });
document.addEventListener("input", publish, { passive: true });
window.addEventListener("pageshow", publish, { passive: true });
publish();
