- **Garra Mobile deixa o icone padrao do template Flutter e passa a usar a
  marca do app, o lobo neon (#1177).** O APK era instalado com o logotipo do
  Flutter porque os `mipmap-*/ic_launcher.png` nunca tinham sido trocados.
  Agora o launcher mostra o mesmo WolfMark do header da home e da splash
  (`lib/widgets/brand/wolf_mark.dart`) como icone adaptativo (API 26+, fundo
  `garra_icon_bg` + frente dentro da zona segura de 66 dp), com camada
  monochrome para os themed icons do Android 13+, `roundIcon` e PNGs legados
  para API < 26 em todas as densidades. Os assets sao gerados por
  `apps/garraia-mobile/tool/gen_launcher_icons.py`, que reproduz a geometria
  do `_WolfPainter` de forma deterministica — rodar o script de novo nao
  produz diff. O papagaio de `assets/logo.png` continua sendo a marca do CLI,
  do desktop e do site.
