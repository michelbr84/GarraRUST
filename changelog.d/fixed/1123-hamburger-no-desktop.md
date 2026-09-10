- **Hamburger do header volta a fazer algo no desktop (#1123).** O botao e
  renderizado em todas as larguras, mas o handler abria o drawer mobile sem
  checar o viewport: no desktop o unico efeito era o overlay que escurece a
  tela, porque a sidebar ja estava visivel pelo layout flex. Agora o clique
  ramifica por `matchMedia('(max-width: 768px)')` - no mobile continua abrindo
  o drawer, no desktop alterna `collapsed` na sidebar (CSS que ja existia e nao
  era usado por JS). `aria-expanded` acompanha o estado, e um listener de
  `resize` limpa `mobile-open` e o overlay ao cruzar o breakpoint, para a gaveta
  nao ficar presa numa janela que cresceu.
