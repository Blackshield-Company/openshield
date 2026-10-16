#!/usr/bin/env python3
"""Generate the OpenShield stock mascot sprite (assets/sprites/blackshield-mercenary.ans).

Procedural pixel-art knight in the Black Steel palette (docs/brand/tokens.css),
exported as truecolor ANSI with half-block characters: a 32x28 px canvas becomes
32x14 terminal cells, two frames (cloak hem and plume sway) separated by
`---frame---`, the format read by src/tui/sprites.rs.

Variants:
  a  plumed sellsword: great helm, blood plume, heater shield with a plain
     Greek cross, planted longsword, crimson cloak (the stock sprite)
  b  shield-wall veteran: barbute, large centred shield, blade over the shoulder

Usage:
  python3 scripts/knight-sprite.py              # write variant a to the stock path
  python3 scripts/knight-sprite.py b -o out.ans # write variant b elsewhere

Edit this script, not the generated art.
"""
import argparse
import os
import sys
W,H=32,28
C=dict(
 o=(10,10,13),      # outline / deep shadow
 d=(34,34,42),      # plate #22222A
 m=(62,67,75),      # mid steel
 s=(110,116,125),   # steel
 S=(154,160,168),   # steel #9AA0A8
 h=(195,199,204),   # highlight #C3C7CC
 b=(228,222,210),   # bone #E4DED2
 R=(193,18,31),     # blood #C1121F
 c=(92,10,16),      # cloak
 C=(58,6,10),       # cloak shade
 k=(130,14,24),     # cloak light
 i=(23,23,29),      # iron #17171D shield field
)
def canvas(): return [[None]*W for _ in range(H)]
def put(g,x,y,col):
    if 0<=x<W and 0<=y<H: g[y][x]=col
def rect(g,x0,y0,x1,y1,col):
    for y in range(y0,y1+1):
        for x in range(x0,x1+1): put(g,x,y,col)
def poly(g,pts,colf):
    ys=[p[1] for p in pts]
    for y in range(int(min(ys)),int(max(ys))+1):
        for x in range(W):
            px,py=x+.5,y+.5; inside=False; j=len(pts)-1
            for i2 in range(len(pts)):
                xi,yi=pts[i2]; xj,yj=pts[j]
                if (yi>py)!=(yj>py) and px<(xj-xi)*(py-yi)/(yj-yi)+xi: inside=not inside
                j=i2
            if inside: put(g,x,y,colf(x,y) if callable(colf) else colf)
def ell(g,cx,cy,rx,ry,colf):
    for y in range(H):
        for x in range(W):
            if ((x+.5-cx)/rx)**2+((y+.5-cy)/ry)**2<=1: put(g,x,y,colf(x,y) if callable(colf) else colf)
def shade(x0,x1,cols):  # left-lit horizontal ramp
    def f(x,y):
        t=(x-x0)/max(1,(x1-x0)); n=len(cols)
        return cols[min(n-1,int(t*n))]
    return f
def outline(g):
    out=[row[:] for row in g]
    for y in range(H):
        for x in range(W):
            if g[y][x] is None:
                for dx,dy in ((1,0),(-1,0),(0,1),(0,-1)):
                    xx,yy=x+dx,y+dy
                    if 0<=xx<W and 0<=yy<H and g[yy][xx] not in (None,'o'):
                        out[y][x]='o'; break
    return out
def heater(g,x0,y0,w,h,cross=True):
    x1=x0+w-1; mid=(x0+x1)/2; split=y0+int(h*.5)
    pts=[(x0,y0),(x1+1,y0),(x1+1,split),(mid+.5,y0+h),(x0,split)]
    poly(g,pts,'S')
    inner=[(x0+1,y0+1),(x1,y0+1),(x1,split),(mid+.5,y0+h-1.6),(x0+1,split)]
    poly(g,inner,'i')
    rect(g,x0+1,y0+1,x0+1,split-1,'d')  # bevel
    if cross:
        cx=int(mid); cy=y0+int(h*.42); a=max(2,w//3); t=max(1,w//8)
        rect(g,cx-t,cy-a,cx+t,cy+a,'R'); rect(g,cx-a,cy-t,cx+a,cy+t,'R')
    for x in range(x0+1,x1+1): put(g,x,y0,'h')

def variant_a(frame):
    g=canvas(); sway=frame
    # cloak behind
    hem=[(5,27),(8,26),(11,27),(15,26),(19,27),(23,26),(26,27)] if not sway else [(6,27),(9,27),(12,26),(16,27),(20,26),(24,27),(27,26)]
    poly(g,[(9,9),(23,9),(27,hem[-1][1])]+hem[::-1]+[(5,hem[0][1])],shade(5,27,['k','c','c','C']))
    # legs
    rect(g,12,21,14,27,'m'); rect(g,12,21,12,27,'S'); rect(g,17,21,19,27,'m'); rect(g,17,21,17,27,'S')
    rect(g,11,26,14,27,'d'); rect(g,17,26,20,27,'d')
    # tasset / skirt
    poly(g,[(11,18),(21,18),(22,22),(10,22)],shade(10,22,['S','s','m','d']))
    rect(g,10,22,22,22,'d')
    # torso
    poly(g,[(11,10),(21,10),(20,18),(12,18)],shade(11,21,['h','S','s','m','d']))
    rect(g,15,11,16,17,'S'); put(g,15,11,'h')
    rect(g,11,18,21,18,'d'); rect(g,15,18,16,18,'b')  # belt + buckle
    # right arm + sword (viewer's right)
    rect(g,21,12,23,16,'m'); rect(g,21,12,21,16,'S')
    rect(g,24,17,24,27,'h'); rect(g,25,17,25,26,'S')   # blade
    rect(g,22,16,27,16,'b')                          # crossguard
    rect(g,24,14,25,15,'d'); rect(g,24,13,25,13,'b')   # grip, pommel
    rect(g,22,14,23,15,'s')                          # gauntlet
    # pauldrons
    ell(g,10.5,11,4,3,shade(6,15,['h','S','s','m']))
    ell(g,21.5,11,4,3,shade(18,26,['S','s','m','d']))
    for x in range(7,15): put(g,x,12,'m')
    for x in range(18,26): put(g,x,12,'d')
    # helm
    poly(g,[(12,4),(20,4),(20,9),(16,10),(12,9)],shade(12,20,['h','S','S','s','m']))
    ell(g,16,4.5,4,2,shade(12,20,['h','S','s','m']))
    rect(g,12,6,19,6,'o'); rect(g,13,6,14,6,'b')       # visor slit + glint
    rect(g,16,7,16,9,'S'); put(g,14,8,'o'); put(g,18,8,'o')  # ridge + breaths
    rect(g,13,10,19,10,'s'); rect(g,13,10,14,10,'S')   # gorget
    # plume
    px=0 if not sway else 1
    for (x,y,col) in [(16,2,'R'),(17,2,'R'),(17,1,'R'),(18,1,'R'),(18+px,0,'R'),(19+px,0,'R'),(19+px,1,'R'),(20+px,1,'c'),(20+px,2,'c'),(18,2,'c'),(16,3,'R'),(17,3,'c')]: put(g,x,y,col)
    # shield (viewer's left)
    heater(g,2,12,11,14)
    return outline(g)

def variant_b(frame):
    g=canvas(); sway=frame
    # cloak: wide, hooded collar
    hem=[(3,27),(7,26),(12,27),(16,26),(20,27),(25,26),(29,27)] if not sway else [(3,26),(7,27),(12,26),(16,27),(20,26),(25,27),(29,26)]
    poly(g,[(8,8),(24,8),(29,hem[-1][1])]+hem[::-1]+[(3,hem[0][1])],shade(3,29,['k','c','c','C','C']))
    ell(g,16,9.5,7,2.2,shade(9,23,['k','c','C']))   # hood collar
    # legs
    rect(g,11,23,13,27,'m'); rect(g,11,23,11,27,'S'); rect(g,18,23,20,27,'m'); rect(g,18,23,18,27,'S')
    # torso
    poly(g,[(10,10),(22,10),(21,22),(11,22)],shade(10,22,['h','S','s','m','d']))
    # sword over right shoulder (viewer's right), pointing up
    rect(g,25,0,25,8,'h'); rect(g,26,1,26,8,'S'); put(g,26,0,'S')
    rect(g,23,9,28,9,'b'); rect(g,25,10,26,11,'d'); rect(g,25,12,26,12,'b')
    # pauldrons: layered, larger
    for i,(cy,ry) in enumerate([(10.5,2.6),(12,2.2),(13.3,1.8)]):
        ell(g,9.5,cy,4.8-i*.6,ry,shade(4,15,['h','S','s','m']))
        ell(g,22.5,cy,4.8-i*.6,ry,shade(18,28,['S','s','m','d']))
    for x in range(5,15): put(g,x,12,'m')
    for x in range(18,28): put(g,x,12,'d')
    # helm: barbute with T-visor
    ell(g,16,5.5,4.2,4.2,shade(12,20,['h','S','S','s','m']))
    rect(g,12,6,20,9,'s'); rect(g,12,6,13,9,'S')
    rect(g,13,7,19,7,'o'); rect(g,16,7,16,9,'o'); put(g,14,7,'b')
    put(g,16,1,'S'); put(g,16,2,'h')                      # crest ridge
    rect(g,16-0+sway,0,16+sway,0,'R')                     # small red tassel tip
    # big center shield
    heater(g,8,13,15,15)
    return outline(g)

def to_ans(g):
    lines=[]
    def col(k): return C[k]
    for y in range(0,H,2):
        s=''
        for x in range(W):
            t=g[y][x]; b=g[y+1][x] if y+1<H else None
            if t is None and b is None: s+=' '; continue
            if t is not None and b is not None:
                if t==b: r,gg,bb=col(t); s+=f'\x1b[38;2;{r};{gg};{bb}m█{{reset}}'
                else:
                    r,gg,bb=col(t); r2,g2,b2=col(b); s+=f'\x1b[38;2;{r};{gg};{bb}m\x1b[48;2;{r2};{g2};{b2}m▀{{reset}}'
            elif t is not None: r,gg,bb=col(t); s+=f'\x1b[38;2;{r};{gg};{bb}m▀{{reset}}'
            else: r,gg,bb=col(b); s+=f'\x1b[38;2;{r};{gg};{bb}m▄{{reset}}'
        lines.append(s.rstrip())
    return lines

VARIANTS = {
    "a": (variant_a, "plumed sellsword: great helm, blood plume, heater shield with a plain Greek cross, planted longsword, crimson cloak"),
    "b": (variant_b, "shield-wall veteran: barbute, large centred heater shield, blade over the shoulder, hooded crimson cloak"),
}
STOCK = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "sprites", "blackshield-mercenary.ans")


def render(variant):
    fn, desc = VARIANTS[variant]
    frames = [fn(0), fn(1)]
    header = (
        f"# blackshield-mercenary: Black Steel knight, {desc}.\n"
        "# 32x14 cells, truecolor half-blocks, two frames (cloak and plume sway).\n"
        "# Palette: docs/brand/tokens.css.\n"
        f"# Generated by scripts/knight-sprite.py (variant {variant}); edit the script, not the art.\n"
    )
    body = "\n---frame---\n".join("\n".join(to_ans(f)) for f in frames)
    return header + body + "\n"


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("variant", nargs="?", default="a", choices=sorted(VARIANTS))
    ap.add_argument("-o", "--output", default=STOCK, help="output path (default: stock sprite)")
    args = ap.parse_args()
    with open(args.output, "w", encoding="utf-8") as fh:
        fh.write(render(args.variant))
    print(f"wrote {os.path.normpath(args.output)}", file=sys.stderr)


if __name__ == "__main__":
    main()
