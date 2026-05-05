/**
 * Escapes a string for safe insertion into innerHTML.
 * Identical to the inline helper in webchat.html.
 */
export function escapeHtml(s) {
  const d = document.createElement("div");
  d.textContent = String(s ?? "");
  return d.innerHTML;
}

/**
 * Escapes a string for safe use inside an HTML attribute value.
 */
export function escapeAttr(s) {
  return String(s ?? "")
    .replace(/&/g, "&amp;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}
