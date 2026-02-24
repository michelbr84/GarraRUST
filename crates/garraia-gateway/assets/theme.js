import { dom } from './dom.js';

window.setLanguage = function (lang) {
  localStorage.setItem("lang", lang);
  if (window.loadLocale) window.loadLocale(lang);
};

const themeStorageKey = "garraia.ui.theme";
const themeOrder = ["light", "dark", "brasil"];

export function getCurrentTheme() {
  const fromDom = document.documentElement.getAttribute("data-theme");
  if (fromDom && themeOrder.includes(fromDom)) return fromDom;
  const stored = localStorage.getItem(themeStorageKey);
  if (stored && themeOrder.includes(stored)) return stored;
  if (window.matchMedia) {
    const prefersDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
    return prefersDark ? "dark" : "light";
  }
  return "light";
}

export function applyTheme(theme, persist = true) {
  const selected = themeOrder.includes(theme) ? theme : "light";
  document.documentElement.setAttribute("data-theme", selected);
  if (persist) localStorage.setItem(themeStorageKey, selected);

  const btn = dom.themeToggleBtn;
  if (btn) {
    const nextTheme = theme === "light" ? "dark" : theme === "dark" ? "brasil" : "light";
    const key = `theme.${nextTheme}_mode`;
    btn.setAttribute("data-i18n", key);
    if (typeof window.t === "function") {
      btn.textContent = window.t(key) || key;
    }
  }
}

function cycleTheme() {
  const current = getCurrentTheme();
  const next = themeOrder[(themeOrder.indexOf(current) + 1) % themeOrder.length];
  applyTheme(next, true);
}

export function initTheme() {
  applyTheme(getCurrentTheme(), false);
  if (dom.themeToggleBtn) {
    dom.themeToggleBtn.removeEventListener("click", cycleTheme);
    dom.themeToggleBtn.addEventListener("click", cycleTheme);
  }
}
