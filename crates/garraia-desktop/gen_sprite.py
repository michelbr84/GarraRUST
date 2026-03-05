"""
Garra parrot sprite sheet generator — v2 (improved animations).

Layout: 3 rows × 8 columns, each frame = 160×200 px
  Row 0 — idle     (4 frames)  teal-green,  gentle bob + wing breathe
  Row 1 — thinking (6 frames)  amber/gold,  head tilt + thought bubbles grow
  Row 2 — talking  (8 frames)  bright-green, beak open/close + sound rings

Output: ui/assets/parrot-sprite.png
"""

import zlib, struct, math, os

FW, FH = 160, 200          # frame size
COLS, ROWS = 8, 3
IW, IH = FW * COLS, FH * ROWS

rgba = bytearray(IW * IH * 4)  # RGBA, all transparent

# ── drawing primitives ──────────────────────────────────────────────────────

def _set(x, y, r, g, b, a):
    if 0 <= x < IW and 0 <= y < IH:
        i = (y * IW + x) * 4
        # Alpha-composite over existing pixel
        src_a = a / 255
        dst_a = rgba[i+3] / 255
        out_a = src_a + dst_a * (1 - src_a)
        if out_a > 0:
            rgba[i]   = int((r * src_a + rgba[i]   * dst_a * (1 - src_a)) / out_a)
            rgba[i+1] = int((g * src_a + rgba[i+1] * dst_a * (1 - src_a)) / out_a)
            rgba[i+2] = int((b * src_a + rgba[i+2] * dst_a * (1 - src_a)) / out_a)
            rgba[i+3] = int(out_a * 255)

def circle(cx, cy, r, col, a=255):
    br, bg, bb = col
    r2 = r * r
    for dy in range(-r, r+1):
        x_span = int(math.sqrt(max(0, r2 - dy*dy)))
        for dx in range(-x_span, x_span+1):
            _set(cx+dx, cy+dy, br, bg, bb, a)

def ellipse(cx, cy, rx, ry, col, a=255):
    br, bg, bb = col
    for dy in range(-ry, ry+1):
        x_span = int(rx * math.sqrt(max(0, 1 - (dy/ry)**2)))
        for dx in range(-x_span, x_span+1):
            _set(cx+dx, cy+dy, br, bg, bb, a)

def rect(x0, y0, w, h, col, a=255):
    br, bg, bb = col
    for dy in range(h):
        for dx in range(w):
            _set(x0+dx, y0+dy, br, bg, bb, a)

def line(x0, y0, x1, y1, r_col, thick=2):
    """Bresenham line with thickness."""
    dx = abs(x1-x0); dy = abs(y1-y0)
    sx = 1 if x0 < x1 else -1; sy = 1 if y0 < y1 else -1
    err = dx - dy
    while True:
        circle(x0, y0, thick, r_col)
        if x0 == x1 and y0 == y1: break
        e2 = 2*err
        if e2 > -dy: err -= dy; x0 += sx
        if e2 <  dx: err += dx; y0 += sy

def gradient_ellipse(cx, cy, rx, ry, col_top, col_bot, a=255):
    """Vertical gradient ellipse for 3D body effect."""
    tr, tg, tb = col_top
    br, bg, bb = col_bot
    for dy in range(-ry, ry+1):
        t = (dy + ry) / (2 * ry)
        r = int(tr + (br-tr)*t); g = int(tg + (bg-tg)*t); b = int(tb + (bb-tb)*t)
        x_span = int(rx * math.sqrt(max(0, 1 - (dy/ry)**2)))
        for dx in range(-x_span, x_span+1):
            _set(cx+dx, cy+dy, r, g, b, a)

# ── parrot parts ────────────────────────────────────────────────────────────

def shadow(ox, oy, bob):
    """Soft drop shadow under feet."""
    ellipse(ox, oy+90+bob, 28, 8, (0,0,0), 60)

def tail(ox, oy, bob, sway):
    """Long tail feathers with sway animation."""
    colors = [(0,140,80), (0,160,100), (20,200,120), (180,220,0)]
    for i, col in enumerate(colors):
        angle = math.radians(-35 + i*25 + sway)
        for seg in range(35):
            tx = ox + int(math.cos(angle) * seg) + (i-2)*4
            ty = oy + 65 + bob + int(math.sin(angle) * seg * 0.4) + seg//2
            circle(tx, ty, max(1, 4-seg//10), col)

def wing(ox, oy, bob, flap, side):
    """Wing with feather detail. side: -1=left, +1=right."""
    base_col = (0, 150, 100)
    tip_col  = (0, 100, 60)
    # flap=0 tucked, 1=mid, 2=raised
    y_offsets = [0, -12, -24]
    y_off = y_offsets[flap]
    wx = ox + side * 32
    wy = oy + 20 + bob + y_off
    # Wing body
    pts = [(wx, wy), (wx+side*30, wy+10), (wx+side*28, wy+38), (wx, wy+42)]
    # Fill wing polygon (approx)
    for seg in range(20):
        t = seg / 19
        # lerp along edges
        x1 = int(pts[0][0] + t*(pts[1][0]-pts[0][0]))
        y1 = int(pts[0][1] + t*(pts[1][1]-pts[0][1]))
        x2 = int(pts[3][0] + t*(pts[2][0]-pts[3][0]))
        y2 = int(pts[3][1] + t*(pts[2][1]-pts[3][1]))
        col = tuple(int(base_col[i] + t*(tip_col[i]-base_col[i])) for i in range(3))
        line(x1, y1, x2, y2, col, thick=3)
    # Feather tips
    for fi in range(4):
        ft = fi / 3
        fx = int(pts[1][0] + ft*(pts[2][0]-pts[1][0]))
        fy = int(pts[1][1] + ft*(pts[2][1]-pts[1][1]))
        line(fx, fy, fx+side*8, fy+12, (0, 80, 50), thick=2)

def body(ox, oy, bob, body_col):
    """3D-shaded body."""
    col_top = tuple(min(255, c+40) for c in body_col)
    col_bot = tuple(max(0, c-30) for c in body_col)
    gradient_ellipse(ox, oy+28+bob, 36, 48, col_top, col_bot)
    # Belly highlight
    ellipse(ox-4, oy+30+bob, 18, 30, (220, 240, 220), 120)

def head(ox, oy, bob, tilt, body_col):
    """Head with tilt (for thinking state)."""
    hx = ox + int(tilt * 8)
    hy = oy - 18 + bob
    col_top = tuple(min(255, c+50) for c in body_col)
    col_bot = tuple(max(0, c-20) for c in body_col)
    gradient_ellipse(hx, hy, 35, 32, col_top, col_bot)
    # Cheek patch
    ellipse(hx+14, hy+8, 12, 10, (255, 100, 80), 180)
    ellipse(hx-14, hy+8, 12, 10, (255, 100, 80), 180)
    return hx, hy

def crest(hx, hy, frame):
    """Animated crest feathers."""
    cols = [(255,50,50), (255,140,0), (255,220,0), (180,255,0)]
    offsets = [0, 2, -2, 1]
    sway = math.sin(frame * 0.8) * 3
    for i, (col, off) in enumerate(zip(cols, offsets)):
        for h in range(10 + i*5):
            fx = hx + off + int(sway * (i+1)/4)
            fy = hy - 28 - h - i*3
            r2 = max(1, 3 - h//6)
            circle(fx, fy, r2, col)

def eyes(hx, hy, blink):
    """Eyes with iris, pupil, highlight. blink=True closes them."""
    for side in [-1, 1]:
        ex, ey = hx + side*14, hy - 6
        circle(ex, ey, 9, (255,255,255))          # sclera
        if blink:
            rect(ex-9, ey-3, 18, 6, (0,160,100)) # eyelid (body color)
        else:
            circle(ex, ey, 6, (30,20,10))         # iris
            circle(ex+1, ey-2, 2, (255,255,255))  # highlight
            circle(ex-2, ey+1, 1, (255,255,255))  # secondary highlight

def beak(hx, hy, open_mouth):
    """Upper + lower beak."""
    # Upper
    for i in range(20):
        xx = hx - 10 + i
        yy = hy + 10 + abs(i-10)//4
        circle(xx, yy, 4, (220,180,0))
    # Lower
    gap = 7 if open_mouth else 0
    for i in range(15):
        xx = hx - 7 + i
        yy = hy + 18 + gap + abs(i-7)//5
        circle(xx, yy, 3, (180,140,0))
    # Nostril
    circle(hx-4, hy+8, 2, (160,120,0))

def feet(ox, oy, bob):
    for side in [-1, 1]:
        fx = ox + side*14
        rect(fx-4, oy+78+bob, 8, 16, (200,160,40))
        for toe in range(3):
            rect(fx-8+toe*6, oy+91+bob, 6, 4, (180,140,30))

def thought_bubbles(hx, hy, frame, n_visible):
    """Growing thought bubbles for thinking state."""
    positions = [(hx+28, hy-30), (hx+42, hy-52), (hx+52, hy-76)]
    sizes     = [4, 7, 11]
    for i in range(min(n_visible, 3)):
        px_, py_ = positions[i]
        sz = sizes[i]
        pulse = int(math.sin(frame * 1.2 + i) * 1.5)
        circle(px_, py_, sz+pulse, (255,255,255), 220)
        circle(px_, py_, sz+pulse-2, (240,240,255), 180)

def sound_rings(hx, hy, frame):
    """Sound wave arcs for talking state."""
    if frame % 2 != 0:
        return
    for wave in range(3):
        r_val = 14 + wave * 10
        fade = 200 - wave * 60
        for deg in range(-60, 61, 5):
            angle = math.radians(deg - 30)
            wx = hx + 38 + int(math.cos(angle) * r_val)
            wy = hy - 5 + int(math.sin(angle) * r_val)
            circle(wx, wy, 2, (255,255,180), fade)

# ── draw one complete parrot frame ─────────────────────────────────────────

STATE_COLORS = {
    'idle':     (10,  170, 120),
    'thinking': (200, 140,  20),
    'talking':  (20,  190,  70),
}

def draw_frame(col, row, state, frame):
    ox = col * FW + FW // 2
    oy = row * FH + FH // 2 - 10
    body_col = STATE_COLORS[state]

    # Breathing bob: subtle up on even frames
    bob = -3 if frame % 2 == 0 else 0

    # Wing flap cycle
    flap_seq = {'idle': [0,0,1,0], 'thinking': [0,0,0,1,1,0], 'talking': [0,1,2,1,0,1,2,1]}
    flap = flap_seq[state][frame % len(flap_seq[state])]

    # Tail sway
    sway = math.sin(frame * 0.9) * 6

    # Head tilt (thinking only)
    tilt = math.sin(frame * 0.7) * 0.15 if state == 'thinking' else 0

    # Blink (thinking: blink on frame 4; idle: occasional blink on frame 3)
    blink = (state == 'thinking' and frame == 4) or (state == 'idle' and frame == 3)

    # Open mouth (talking: alternate frames)
    open_mouth = state == 'talking' and frame % 2 == 0

    # ── render layers (back to front) ──
    shadow(ox, oy, bob)
    tail(ox, oy, bob, sway)
    wing(ox, oy, bob, flap, -1)
    wing(ox, oy, bob, flap,  1)
    body(ox, oy, bob, body_col)
    feet(ox, oy, bob)
    hx, hy = head(ox, oy, bob, tilt, body_col)
    crest(hx, hy, frame)
    eyes(hx, hy, blink)
    beak(hx, hy, open_mouth)

    # State-specific FX
    if state == 'thinking':
        n = min(3, frame // 2 + 1)
        thought_bubbles(hx, hy, frame, n)
    elif state == 'talking':
        sound_rings(hx, hy, frame)

# ── render all states ───────────────────────────────────────────────────────

FRAME_COUNTS = [('idle', 4), ('thinking', 6), ('talking', 8)]

for row, (state, n) in enumerate(FRAME_COUNTS):
    for col in range(n):
        draw_frame(col, row, state, col)

# ── encode & save PNG ───────────────────────────────────────────────────────

def encode_png(w, h, data):
    def chunk(tag, d):
        c = zlib.crc32(tag + d) & 0xFFFFFFFF
        return struct.pack('>I', len(d)) + tag + d + struct.pack('>I', c)
    ihdr = struct.pack('>IIBBBBB', w, h, 8, 6, 0, 0, 0)
    raw = b''.join(b'\x00' + bytes(data[(y*w*4):(y*w*4+w*4)]) for y in range(h))
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', ihdr) + chunk(b'IDAT', zlib.compress(raw, 6)) + chunk(b'IEND', b'')

out = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'ui', 'assets', 'parrot-sprite.png')
os.makedirs(os.path.dirname(out), exist_ok=True)
png = encode_png(IW, IH, rgba)
with open(out, 'wb') as f:
    f.write(png)

print(f"Sprite: {IW}x{IH}px  {len(png)//1024}KB  saved to: {out}")
