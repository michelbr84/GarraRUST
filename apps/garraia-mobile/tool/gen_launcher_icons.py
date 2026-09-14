#!/usr/bin/env python3
"""Gera os icones de launcher Android do Garra Mobile a partir do WolfMark.

O app tem marca propria: o lobo geometrico de `lib/widgets/brand/wolf_mark.dart`
(header da home, splash), tracado em neon violeta/magenta com olho ciano — o
proprio doc do widget diz que ele "doubles as the source for the adaptive
launcher icon". Este script e essa fonte: reproduz a geometria do
`_WolfPainter` (poligono da cabeca e facetas numa caixa de 100x100) com Pillow,
para o launcher mostrar a mesma marca que a home mostra (#1177). O papagaio de
`assets/logo.png` continua sendo a marca do CLI, do desktop e do site.

Saidas (todas em `android/app/src/main/res/`):

- `mipmap-anydpi-v26/ic_launcher.xml` + `ic_launcher_round.xml`: icone
  adaptativo (API 26+) — fundo `@color/garra_icon_bg`, frente
  `@mipmap/ic_launcher_foreground`, camada `<monochrome>` para os themed
  icons do Android 13+.
- `mipmap-{m,h,x,xx,xxx}hdpi/ic_launcher_foreground.png`: a frente, canvas
  de 108 dp com o lobo dentro da zona segura de 66 dp.
- `mipmap-{m,h,x,xx,xxx}hdpi/ic_launcher_monochrome.png`: silhueta branca
  da cabeca (o Android tinge pela paleta do tema).
- `mipmap-{m,h,x,xx,xxx}hdpi/ic_launcher.png` + `ic_launcher_round.png`:
  icones legados (API < 26) de 48 dp, ja compostos sobre o fundo.

Uso (de qualquer diretorio):

    python3 apps/garraia-mobile/tool/gen_launcher_icons.py

Requer Pillow (`pip install pillow`). Deterministico: rodar duas vezes gera
bytes identicos, entao um diff nos PNGs significa que a geometria, as cores
ou este script mudaram.
"""

from __future__ import annotations

import sys
from pathlib import Path

try:
    from PIL import Image, ImageChops, ImageDraw, ImageFilter
except ImportError:  # pragma: no cover - dependencia de ferramenta, nao do app
    sys.exit("Pillow ausente: pip install pillow")

HERE = Path(__file__).resolve()
APP_DIR = HERE.parents[1]  # apps/garraia-mobile
RES = APP_DIR / "android" / "app" / "src" / "main" / "res"

# ── Cores: espelho de lib/theme/garra_tokens.dart (GarraColors) ─────────────
VIOLET_LIGHT = (0xA7, 0x8B, 0xFA)
VIOLET = (0x8B, 0x5C, 0xF6)
MAGENTA = (0xFF, 0x3F, 0xA4)
CYAN = (0x16, 0xD9, 0xFF)
# Mesmo valor de `res/values/colors.xml` (`garra_icon_bg`). Repetido aqui so
# para compor os PNGs legados, que nao conseguem referenciar o recurso.
ICON_BG = (0x1B, 0x17, 0x47, 0xFF)

# ── Geometria: espelho de _WolfPainter em wolf_mark.dart (caixa 100x100) ───
HEAD = [
    (22, 14),  # left ear tip
    (38, 40),  # ear base (inner)
    (54, 34),  # brow
    (62, 10),  # right ear tip
    (78, 40),  # right ear base
    (88, 52),  # crown -> cheek
    (76, 78),  # jaw
    (56, 92),  # chin
    (34, 84),  # muzzle underside
    (8, 70),  # snout tip
    (22, 58),  # snout top
    (26, 42),  # forehead
]
FACETS = [
    [(40, 50), (58, 46), (70, 54)],  # brow ridge
    [(24, 62), (44, 70), (60, 80)],  # muzzle crease
]
EYE = (50, 58)

# Densidades Android: dp -> px = dp * fator.
DENSITIES = {
    "mdpi": 1.0,
    "hdpi": 1.5,
    "xhdpi": 2.0,
    "xxhdpi": 3.0,
    "xxxhdpi": 4.0,
}

ADAPTIVE_DP = 108  # canvas total do icone adaptativo
SAFE_DP = 66  # zona segura: o que sobrevive a qualquer mascara do launcher
LEGACY_DP = 48
# Supersampling: tudo e desenhado em 4x e reduzido com LANCZOS — o Pillow nao
# tem antialias nativo em `line`/`polygon`.
SS = 4


def _lerp(a: tuple[int, int, int], b: tuple[int, int, int], t: float) -> tuple[int, int, int]:
    return tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3))  # type: ignore[return-value]


def _gradient(size: int) -> Image.Image:
    """Gradiente linear topo-esquerda -> base-direita em tres paradas
    (violetLight -> violet -> magenta), como o `LinearGradient` do widget."""
    grad = Image.new("RGB", (size, size))
    px = grad.load()
    denom = 2 * (size - 1) if size > 1 else 1
    for y in range(size):
        for x in range(size):
            t = (x + y) / denom
            if t < 0.5:
                c = _lerp(VIOLET_LIGHT, VIOLET, t * 2)
            else:
                c = _lerp(VIOLET, MAGENTA, (t - 0.5) * 2)
            px[x, y] = c
    return grad


def _stroke_mask(size: int, k: float, offset: float, width: float, closed: bool, pts) -> Image.Image:
    """Mascara (L) de um tracado com juntas/pontas redondas, desenhado a `SS`x."""
    mask = Image.new("L", (size * SS, size * SS), 0)
    draw = ImageDraw.Draw(mask)
    w = max(1, round(width * SS))
    scaled = [((x * k + offset) * SS, (y * k + offset) * SS) for x, y in pts]
    seq = scaled + [scaled[0]] if closed else scaled
    draw.line(seq, fill=255, width=w, joint="curve")
    # `joint="curve"` nao arredonda as pontas de um caminho aberto nem a junta
    # do fechamento; os discos nas vertices cobrem os dois casos.
    r = w / 2
    for x, y in scaled:
        draw.ellipse((x - r, y - r, x + r, y + r), fill=255)
    return mask.resize((size, size), Image.LANCZOS)


def _fill_mask(size: int, k: float, offset: float, pts) -> Image.Image:
    mask = Image.new("L", (size * SS, size * SS), 0)
    ImageDraw.Draw(mask).polygon([((x * k + offset) * SS, (y * k + offset) * SS) for x, y in pts], fill=255)
    return mask.resize((size, size), Image.LANCZOS)


def _tinted(size: int, mask: Image.Image, color: Image.Image | tuple, alpha: float = 1.0) -> Image.Image:
    """Camada RGBA: `color` (imagem RGB ou tupla) recortada por `mask`,
    com alfa global `alpha`."""
    if isinstance(color, tuple):
        layer = Image.new("RGBA", (size, size), (*color, 255))
    else:
        layer = color.convert("RGBA")
    if alpha < 1.0:
        mask = mask.point(lambda v: round(v * alpha))
    layer.putalpha(mask)
    return layer


def wolf(size: int, box: float, glow: bool = True) -> Image.Image:
    """O WolfMark num canvas quadrado de `size` px, com a caixa de desenho de
    `box` px centralizada. Segue `_WolfPainter.paint` camada a camada:
    glow, preenchimento translucido, contorno, facetas, olho."""
    k = box / 100.0
    offset = (size - box) / 2
    stroke = max(1.6, box * 0.055)
    grad = _gradient(size)
    out = Image.new("RGBA", (size, size), (0, 0, 0, 0))

    if glow:
        glow_mask = _stroke_mask(size, k, offset, stroke * 1.6, True, HEAD).filter(
            ImageFilter.GaussianBlur(box * 0.09)
        )
        out.alpha_composite(_tinted(size, glow_mask, grad))

    # Preenchimento: gradiente vertical do primeiro ao ultimo tom, 28% -> 10%.
    fill_grad = Image.new("RGBA", (size, size))
    fpx = fill_grad.load()
    for y in range(size):
        t = y / max(1, size - 1)
        c = _lerp(VIOLET_LIGHT, MAGENTA, t)
        a = round(255 * (0.28 + (0.10 - 0.28) * t))
        for x in range(size):
            fpx[x, y] = (*c, a)
    # Alfa final = forma da cabeca x alfa do gradiente.
    fill_layer = fill_grad.copy()
    fill_layer.putalpha(ImageChops.multiply(_fill_mask(size, k, offset, HEAD), fill_grad.getchannel("A")))
    out.alpha_composite(fill_layer)

    out.alpha_composite(_tinted(size, _stroke_mask(size, k, offset, stroke, True, HEAD), grad))
    for facet in FACETS:
        out.alpha_composite(_tinted(size, _stroke_mask(size, k, offset, stroke * 0.7, False, facet), grad))

    # Olho: ponto ciano com glow proprio.
    ex, ey = EYE[0] * k + offset, EYE[1] * k + offset
    eye_glow = Image.new("L", (size * SS, size * SS), 0)
    r = box * 0.05 * SS
    ImageDraw.Draw(eye_glow).ellipse((ex * SS - r, ey * SS - r, ex * SS + r, ey * SS + r), fill=255)
    eye_glow = eye_glow.resize((size, size), Image.LANCZOS).filter(ImageFilter.GaussianBlur(box * 0.05))
    out.alpha_composite(_tinted(size, eye_glow, CYAN, alpha=0.6))
    eye = Image.new("L", (size * SS, size * SS), 0)
    r = box * 0.032 * SS
    ImageDraw.Draw(eye).ellipse((ex * SS - r, ey * SS - r, ex * SS + r, ey * SS + r), fill=255)
    out.alpha_composite(_tinted(size, eye.resize((size, size), Image.LANCZOS), CYAN))
    return out


def foreground(px_per_dp: float) -> Image.Image:
    canvas_px = round(ADAPTIVE_DP * px_per_dp)
    # A caixa de desenho ocupa ~88% da zona segura: o glow e as orelhas ficam
    # inteiros sob qualquer mascara de launcher.
    return wolf(canvas_px, SAFE_DP * px_per_dp * 0.88)


def monochrome(px_per_dp: float) -> Image.Image:
    """Silhueta branca da cabeca, sem glow nem facetas: o Android tinge pela
    paleta do tema (Material You), so a forma importa."""
    size = round(ADAPTIVE_DP * px_per_dp)
    box = SAFE_DP * px_per_dp * 0.88
    k = box / 100.0
    offset = (size - box) / 2
    stroke = max(1.6, box * 0.055)
    mask = ImageChops.lighter(
        _fill_mask(size, k, offset, HEAD),
        _stroke_mask(size, k, offset, stroke, True, HEAD),
    )
    return _tinted(size, mask, (255, 255, 255))


def legacy(px_per_dp: float, round_mask: bool) -> Image.Image:
    size = round(LEGACY_DP * px_per_dp)
    # Fundo: quadrado de cantos arredondados (ou circulo), na cor do icone
    # adaptativo — mesma leitura visual nos dois mundos.
    bg = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    mask = Image.new("L", (size * SS, size * SS), 0)
    draw = ImageDraw.Draw(mask)
    if round_mask:
        draw.ellipse((0, 0, size * SS - 1, size * SS - 1), fill=255)
    else:
        draw.rounded_rectangle((0, 0, size * SS - 1, size * SS - 1), radius=round(size * SS * 0.20), fill=255)
    mask = mask.resize((size, size), Image.LANCZOS)
    bg.paste(Image.new("RGBA", (size, size), ICON_BG), (0, 0), mask)
    # No legado nao ha zona segura: o lobo pode ocupar mais do canvas.
    bg.alpha_composite(wolf(size, size * 0.70))
    return bg


def write_png(img: Image.Image, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    # `optimize` + sem metadados: bytes estaveis entre execucoes.
    img.save(path, format="PNG", optimize=True)


ADAPTIVE_XML = """<?xml version="1.0" encoding="utf-8"?>
<!-- Gerado por tool/gen_launcher_icons.py (#1177). Icone adaptativo: o
     launcher aplica a propria mascara sobre fundo + frente; a camada
     monochrome alimenta os themed icons do Android 13+. -->
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
    <background android:drawable="@color/garra_icon_bg"/>
    <foreground android:drawable="@mipmap/ic_launcher_foreground"/>
    <monochrome android:drawable="@mipmap/ic_launcher_monochrome"/>
</adaptive-icon>
"""


def main() -> None:
    for density, factor in DENSITIES.items():
        d = RES / f"mipmap-{density}"
        write_png(foreground(factor), d / "ic_launcher_foreground.png")
        write_png(monochrome(factor), d / "ic_launcher_monochrome.png")
        write_png(legacy(factor, round_mask=False), d / "ic_launcher.png")
        write_png(legacy(factor, round_mask=True), d / "ic_launcher_round.png")
    anydpi = RES / "mipmap-anydpi-v26"
    anydpi.mkdir(parents=True, exist_ok=True)
    (anydpi / "ic_launcher.xml").write_text(ADAPTIVE_XML, encoding="utf-8")
    (anydpi / "ic_launcher_round.xml").write_text(ADAPTIVE_XML, encoding="utf-8")
    print(f"icones gerados em {RES.relative_to(APP_DIR.parents[1])}")


if __name__ == "__main__":
    main()
