// Sistema de Internacionalização (i18n) do GarraIA
// Suporte a múltiplos idiomas com fallback para inglês

let currentLocale = "pt-BR";
let translations = {};
let fallbackTranslations = {};

// Carrega o idioma selecionado
async function loadLocale(lang = "pt-BR") {
    currentLocale = lang;
    
    // Carrega o idioma principal
    try {
        const response = await fetch(`/assets/locales/${lang}.json`);
        if (response.ok) {
            translations = await response.json();
        } else {
            console.warn(`Failed to load locale: ${lang}, using fallback`);
            translations = {};
        }
    } catch (e) {
        console.error(`Error loading locale ${lang}:`, e);
        translations = {};
    }
    
    // Carrega fallback em inglês se for diferente
    if (lang !== "en-US") {
        try {
            const response = await fetch("/assets/locales/en-US.json");
            if (response.ok) {
                fallbackTranslations = await response.json();
            }
        } catch (e) {
            fallbackTranslations = {};
        }
    } else {
        fallbackTranslations = {};
    }

    // Aplica as traduções na página
    applyTranslations();
}

// Função principal de tradução
function t(key, params = {}) {
    let text = translations[key] || fallbackTranslations[key] || key;
    
    // Substitui parâmetros como ${id}, ${count}, etc.
    Object.keys(params).forEach(param => {
        text = text.replace(new RegExp(`\\$\\{${param}\\}`, 'g'), params[param]);
    });
    
    return text;
}

// Aplica traduções a todos os elementos com data-i18n
function applyTranslations() {
    // Elementos de texto
    document.querySelectorAll("[data-i18n]").forEach(el => {
        const key = el.getAttribute("data-i18n");
        el.innerText = t(key);
    });

    // Elementos de placeholder
    document.querySelectorAll("[data-i18n-placeholder]").forEach(el => {
        const key = el.getAttribute("data-i18n-placeholder");
        el.placeholder = t(key);
    });

    // Elementos de title/tooltip
    document.querySelectorAll("[data-i18n-title]").forEach(el => {
        const key = el.getAttribute("data-i18n-title");
        el.title = t(key);
    });
}

// Atualiza uma tradução específica (útil para mensagens dinâmicas)
function updateTranslation(key, params = {}) {
    return t(key, params);
}

// Disponibiliza funções globalmente (acesso direto)
window.loadLocale = loadLocale;
window.t = t;
window.applyTranslations = applyTranslations;
window.updateTranslation = updateTranslation;

// Disponibiliza também como objeto window.i18n (para código mais organizado)
window.i18n = {
    loadLocale,
    t,
    applyTranslations,
    updateTranslation,
    getLocale: () => currentLocale
};

// Exporta para uso em módulos
if (typeof module !== 'undefined' && module.exports) {
    module.exports = { loadLocale, t, applyTranslations };
}
