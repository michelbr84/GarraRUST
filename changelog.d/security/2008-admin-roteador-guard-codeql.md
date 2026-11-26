- **Alerta CodeQL #17 reaberto e fechado pelo codigo (#2008).** O #1998
  corrigiu o roteador do Web Console, mas o CodeQL seguia acusando
  `pages[page]()` em `admin.html`: a forma reflexiva
  `Object.prototype.hasOwnProperty.call` nao e barreira para
  `js/unvalidated-dynamic-method-call`, e o padrao
  `if (!guard) page = 'dashboard'` nao sanitiza o uso depois do `if`. A
  dispensa como falso-positivo, de 2026-05-12, descrevia como safelist o
  filtro que o #1996 provou furado; o alerta foi reaberto em vez de ganhar
  justificativa nova. O filtro virou
  `page = Object.hasOwn(pages, page) ? page : 'dashboard';`, com o mesmo
  comportamento e reconhecido pelo CodeQL 2.27.6. O painel passa a exigir
  Chrome 93, Firefox 92 ou Safari 15.4 (2021-2022).
