#!/usr/bin/env python3
"""Generate the OpenShield stock mascot sprite (assets/sprites/blackshield-mercenary.ans).

Procedural pixel-art knight in the Black Steel palette (docs/brand/tokens.css),
exported as truecolor ANSI with half-block characters: a 32x28 px canvas becomes
32x14 terminal cells, two frames (cloak hem and plume sway) separated by
`---frame---`, the format read by src/tui/sprites.rs.

The stock sprite (variant a) carries five animation states, each a group of
frames under a `---state:<name>---` marker: idle (breathing, plume and cloak
sway), thinking, working (sword ready and strike), success (shield raised)
and error (stagger with a red flash).

Variants:
  a  plumed sellsword (the stock sprite, reworked 2026-10-18 to match
     brand/reference/knight-sprite-reference.png): blackened, battered plate,
     great helm with a narrow eye slit and a long red plume, ragged crimson fur
     mantle and tattered cloak, a straight longsword upright in his RIGHT hand
     (viewer's left), a battered black heater shield with a worn red cross
     pattee on his LEFT arm (viewer's right) with a plain black-steel rim,
     ember rim-light on the pauldron and arm
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
YPAD=0   # extra rows of headroom above the knight (idle_smooth / banner use 1 with H=30)
C=dict(
 o=(8,8,10),        # outline
 n=(20,20,25),      # blackened plate, deep
 d=(34,34,42),      # plate #22222A
 m=(56,58,66),      # battered edge
 s=(98,102,110),    # dull steel (sparse)
 S=(140,144,150),   # blade
 h=(190,192,194),   # glint
 b=(228,222,210),   # bone #E4DED2
 R=(193,18,31),     # blood #C1121F
 k=(140,20,26),     # plume / cloak light
 c=(92,10,16),      # cloak
 C=(52,6,10),       # cloak shade
 f=(70,14,16),      # fur mantle
 v=(13,13,16),      # shield field, black
 e=(168,72,28),     # ember rim light
 r=(84,78,70),      # iron rim rivets
)
# variant b keeps its original palette
CB=dict(o=(10,10,13),d=(34,34,42),m=(62,67,75),s=(110,116,125),S=(154,160,168),h=(195,199,204),
        b=(228,222,210),R=(193,18,31),c=(92,10,16),C=(58,6,10),k=(130,14,24),i=(23,23,29))
OFF=[0,0]
def canvas(): return [[None]*W for _ in range(H)]
def put(g,x,y,col):
    x+=OFF[0]; y+=OFF[1]+YPAD
    if 0<=x<W and 0<=y<H: g[y][x]=col
def tint(g,x,y,col,skip=(None,'o','R','k','c','C','f')):
    """Recolour an existing pixel (after outlining), using the same offsets as put()."""
    x+=OFF[0]; y+=OFF[1]+YPAD
    if 0<=x<W and 0<=y<H and g[y][x] not in skip: g[y][x]=col
def rect(g,x0,y0,x1,y1,col):
    for y in range(y0,y1+1):
        for x in range(x0,x1+1): put(g,x,y,col)
def stamp(g,x0,y0,rows):
    for dy,row in enumerate(rows):
        for dx,ch in enumerate(row):
            if ch!='.': put(g,x0+dx,y0+dy,ch)
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
    for y in range(len(g)):
        for x in range(len(g[0])):
            if g[y][x] is None:
                for dx,dy in ((1,0),(-1,0),(0,1),(0,-1)):
                    xx,yy=x+dx,y+dy
                    if 0<=xx<len(g[0]) and 0<=yy<len(g) and g[yy][xx] not in (None,'o'):
                        out[y][x]='o'; break
    return out

# ---- parts (ASCII stamps, '.' = transparent; stamped at absolute x0,y0) ----
CLOAK=(2,10,[  # tattered crimson cloak behind the body, wind to viewer's right
 ".......CcccccccccccccC.........",
 "......CccccccccccccccccC.......",
 ".....CcccccccccccccccccccC.....",
 ".....CcccccccccccccccccccCC....",
 "....CcccccccccccccccccccccCC...",
 "....CccccccccccccccccccccccC...",
 "....CcccccccccccccccccccccccC..",
 "...CccccccccccccccccccccccccC..",
 "...CccccccccccccccccccccccccCC.",
 "...CcccccccccccccccccccccccckC.",
 "..CccccccccccccccccccccccccckC.",
 "..CccccccccccccccccccccccccckCC",
 "..CcccccccccccccccccccccccccckC",
 ".CccccccccccccccccccccccccccckC",
 ".CcccCccccccccccccccccccccccCkC",
 ".CcC.CcC..cccccccccccccccc.Cc.C",
 ".C...C.....................C...",
 "..............................."])
CLOAK_HEM=[".CcC.CcC..cccccccccccccccc.Cc.C",".C...C.....................C..."]
CLOAK_HEM2=["CcC.CcCC..cccccccccccccccc.CcC.","C...C.......................C.."]
LEGS=(11,20,[
 "nmdn..nddn",
 "nmdn..nddn",
 "nmdn..nddn",
 "nmdd..nddn",
 "nmsdn.nddn",
 "nmddn.ndddn",
 "ndddnnnnddn",
 "nnnnn.nnnnn"])
TABARD=(13,19,["cRRRc","ckRcC","ckRcC","CkccC",".kcC.",".CcC."])
TORSO=(11,10,[  # blackened breastplate with a battered central ridge
 "..nnddddnn",
 ".nmmddmddnn",
 "nmmsdmdddnn",
 "nmsddmddddn",
 "nmddddmdddn",
 "nmdddmddddn",
 "nmdddmdddnn",
 ".nmddmddnn.",
 "nCCCmsmCCCn",
 "nCcCCmCCCCn"])
HELM=(12,1,[  # great helm: flat-topped dome, one narrow eye slit, breath holes
 "..nmmdn.",
 ".nmsmddn",
 "nmsmmddn",
 "nmsmdddn",
 "oooooooo",
 "nmmdmddn",
 "nmdnmodn",
 "nmmnmodn",
 ".nmdmdn.",
 "..nddn.."])
PLUMES=[  # long red plume streaming from the crest to viewer's right
 (13,0,["..okRRkRk.......",
        "....ocRRRRkk....",
        ".......cRRRRk...",
        ".........ccRRk..",
        "...........cRRk.",
        "............cRk.",
        "............ckc.",
        ".............c.."]),
 (13,0,["..okRRkRkk......",
        "....ocRRRRRk....",
        "........ccRRRk..",
        "..........ccRRk.",
        "............cRRk",
        ".............cRk",
        ".............c.c",
        "..............c."]),
 (13,0,["..okRRkRk.......",   # plume=-1: swayed back toward the helm
        "....ocRRRRk.....",
        "......cRRRRk....",
        "........ccRRk...",
        "..........cRRk..",
        "...........cRk..",
        "...........ck...",
        "............c..."]),
]
MANTLE=(6,8,[  # ragged crimson fur mantle over the pauldrons
 "......fkf...fkf......",
 "...fkRkcf....fckRkf..",
 "..fkRcfnf....fncfkRf.",
 ".fkcfnnmn....nmnfckkf",
 ".kcfnmsdn....nddnfcck",
 ".fcnmsddn....nddddnfc",
 "..fnmddn......nddnnf.",
 "..k.nnf........fnn.k.",
 "....f............f..."])
ARM_R=(5,13,[  # sword arm, bent, gauntlet on the grip
 "..nmdn.",
 "..nmdn.",
 ".nmsdn.",
 ".nmddn.",
 "nmsmdn.",
 "nmddmn.",
 "nmdmdn.",
 ".nddn.."])
SWORD=(3,0,[  # longsword: perfectly vertical 3px blade centred on x=6, tapering to a sharp 1px point; crossguard, grip, fist and pommel centred on it
 "...S...",
 "...S...",
 "..mSm..",
 "..SSm..",
 "..Ssm..",
 "..Ssm..",
 "..Ssm..",
 "..Ssm..",
 "..Ssm..",
 "..Ssm..",
 "..Ssm..",
 "..Ssm..",
 "..Ssm..",
 "..Ssm..",
 "..Ssm..",
 "..Ssm..",
 "smsmmsm",
 "...n...",
 "...n...",
 "...n...",
 "...m...",
 "..nmn.."])
FIST=(3,17,["nmssmmn","nmddmdn",".nnnnn."])  # gauntlet closed over the grip, centred on the blade
SHIELD_X,SHIELD_Y=18,11
SHIELD=[  # blackened, battered heater: same n/d/m/s ramp as breastplate and helm, left-lit;
          # plain black-steel rim like the pauldrons (m/s lit, n shaded), v = inner bevel shadow, s = rivets.
          # No red, crimson or ember anywhere on the trim.
          # The LEFT rim is one unbroken 'm' line (rivets sit just inside it) so the edge never reads as cracked.
 "smmsmmdmdddn",
 "mvvvvvvvvvvn",
 "msdmnnnnvnns",
 "mvddmnnnmnnn",
 "mvdnnnnnnnnn",
 "mvddnnnnnnvd",
 "mvdnnnnnnvnn",
 "msdnnnnnnnns",
 "mvddvnnnnvnn",
 ".mvdsnnnmvn.",
 ".mvdnnnnnmn.",
 ".msdnnnnnvs.",
 "..mvdnnnvn..",
 "...mvnnvn...",
 "....mvvn....",
 ".....mn.....",
]
CROSS=[  # cross pattee (Templar), old paint on steel: cloak crimsons k/c/C following the shield's left-lit ramp,
         # one sparing R highlight, chips where the paint is gone (d/n steel, m scratch)
 "..kkcc..",
 "...kc...",
 "...km...",
 "k..kc..c",
 "kkkRkcnc",
 "kckccccC",
 "k..kc..C",
 "...kc...",
 "...dc...",
 "...kc...",
 "..nkcC..",
]
PROTECT=set()   # canvas pixels (shield + helm and their dark rings) kept plain steel: no ember, no error flash
def _protect(x,y):
    PROTECT.add((x+OFF[0],y+OFF[1]+YPAD))
RING=((-1,0),(1,0),(0,1),(0,-1),(-1,1),(1,1),(-1,-1),(1,-1))
def heater(g,x0,y0,ember='right'):
    """Battered black heater shield. The whole rim is plain blackened steel (m/s/d/n): no red, no ember.
    A dark separation ring (all sides, incl. the top edge under the mantle) keeps the crimson mantle and
    cloak off the trim. `ember` is accepted for API compatibility; the shield takes no rim-light."""
    for dy,row in enumerate(SHIELD):
        for dx,ch in enumerate(row):
            if ch!='.':
                for ex,ey in RING:
                    xx,yy=dx+ex,dy+ey
                    if not (0<=yy<len(SHIELD) and 0<=xx<len(SHIELD[0]) and SHIELD[yy][xx]!='.'):
                        put(g,x0+xx,y0+yy,'o'); _protect(x0+xx,y0+yy)
                _protect(x0+dx,y0+dy)
    stamp(g,x0,y0,SHIELD)
    stamp(g,x0+2,y0+1,CROSS)
    put(g,x0+6,y0,'n')                              # nick in the top rim


# ---- sword poses (all drawn with the right hand, viewer's left) ----
SWORD_READY=(3,0,SWORD[2][:4]+SWORD[2][8:])  # raised: hilt at the shoulder, blade straight up beside the helm, point kept in frame
FIST_READY=(3,13,FIST[2])
ARM_READY=(5,10,ARM_R[2])
ARM_STRIKE=(9,10,["nmmdn.",
                  "nmsddn",
                  ".nmddn"])
STRIKE=[  # level backhand cut outward to the viewer's left; the blade never crosses the shield
 (8,12,["s","m","s","m","m","s"]),          # crossguard (vertical, swung)
 (2,13,["SSSSSS"]),                         # straight 3px blade with a sharp point at x=0 (viewer's left)
 (0,14,["sSSShSSS"]),
 (2,15,["mmmmmm"]),
 (9,13,["nmsm","nmdm","nnnn"]),             # fist
 (13,14,["mn"]),                            # pommel
]
STRIKE_GLINTS=((2,12),(5,11),(1,17),(6,17))

def S(g,part): stamp(g,part[0],part[1],part[2])
def knight_a(p):
    """Draw knight A from a pose dict (see POSES).
    Keys: breath, head (px down), plume (-1/0/1 sway), hem (0/1), sword ('planted'|'ready'|'strike'),
    shield_up (px), dots (thinking dots), dx (shake), flash (error tint), ember ('right'|'left': rim-light side)."""
    g=canvas(); ember=p.get('ember','right'); PROTECT.clear()
    by=p.get('breath',0); hy=by+p.get('head',0); su=p.get('shield_up',0)
    sword=p.get('sword','planted')
    OFF[:]=[0,0]
    x0,y0,rows=CLOAK
    stamp(g,x0,y0,rows[:15]+(CLOAK_HEM2 if p.get('hem',0) else CLOAK_HEM))
    S(g,LEGS)
    OFF[:]=[0,by]
    S(g,TABARD); S(g,TORSO)
    if sword=='planted':
        S(g,ARM_R); S(g,MANTLE); S(g,SWORD); S(g,FIST)   # sword in front of the mantle so the blade stays unbroken
    elif sword=='ready':
        S(g,ARM_READY); S(g,MANTLE); S(g,SWORD_READY); S(g,FIST_READY)
    else:
        S(g,MANTLE); S(g,ARM_STRIKE)
    OFF[:]=[0,hy]
    pl=p.get('plume',0)
    S(g,PLUMES[1 if pl>0 else (2 if pl<0 else 0)])
    S(g,HELM)                                       # helm over the plume root: the helm face is all black steel
    hx,hy0,hrows=HELM
    for ry,row in enumerate(hrows):
        for rx,ch in enumerate(row):
            if ch!='.':
                for ex,ey in ((0,0),)+RING: _protect(hx+rx+ex,hy0+ry+ey)   # helm + its outline ring
    OFF[:]=[0,by-su]                                # shield is held on the left arm: it bobs and shifts with the body every frame
    heater(g,SHIELD_X,SHIELD_Y,ember)              # after the helm: nothing ever cuts the shield's left rim
    OFF[:]=[0,by]
    if sword=='strike':
        for part in STRIKE: S(g,part)
        for x,y in STRIKE_GLINTS: put(g,x,y,'b')     # motion glints
    OFF[:]=[0,hy]
    for x,y in ((9,3),(11,2),(13,1))[:p.get('dots',0)]:   # thinking dots, rising
        put(g,x,y,'b')
    OFF[:]=[0,0]
    if su>=4:
        for x,y in ((31,6),(30,2),(21,5)): put(g,x,y,'b')   # glints off the raised shield
    g=outline(g)
    # ember rim light, back-lit (right) or facing a fire on the viewer's left
    if ember=='right': pts=[(26,11,by),(10,14,by)]   # helm and shield take no ember (plain black steel)
    else: pts=[(5,17,by),(5,18,by)]
    for x,y,oy in pts:
        OFF[:]=[0,oy]
        if (x,y+oy+YPAD) in PROTECT: continue          # never on the shield rim or helm
        if sword=='planted' or (x,y) not in ((10,14),(5,17),(5,18)): tint(g,x,y,'e')
    OFF[:]=[0,0]
    dx=p.get('dx',0)
    if dx:
        g=[([None]*dx+row[:-dx]) if dx>0 else (row[-dx:]+[None]*(-dx)) for row in g]
    if p.get('flash'):
        prot={(x+dx,y) for x,y in PROTECT}
        g=[[(k if (x,y) in prot else ('R' if k=='o' else ('k' if k in ('s','S','h') else ('c' if k=='m' else k))))
            for x,k in enumerate(row)] for y,row in enumerate(g)]
    return g


def variant_a(frame):
    return knight_a({'hem': frame, 'plume': frame})


# Animation states for knight A: state -> list of poses (one per frame).
POSES = {
    "idle": [
        {"breath": 0, "plume": 0, "hem": 0},
        {"breath": 0, "plume": 1, "hem": 0},
        {"breath": 1, "plume": 1, "hem": 1},
        {"breath": 1, "plume": 0, "hem": 1},
    ],
    "thinking": [
        {"head": 1, "dots": 1},
        {"head": 1, "dots": 2, "plume": 1},
        {"head": 1, "dots": 3, "plume": 1},
        {"head": 1, "dots": 0},
    ],
    "working": [
        {"sword": "planted"},
        {"sword": "ready", "plume": 1},
        {"sword": "strike", "plume": 1, "hem": 1},
        {"sword": "ready", "hem": 1},
    ],
    "success": [
        {"shield_up": 2},
        {"shield_up": 4, "plume": 1},
        {"shield_up": 4, "plume": 0, "hem": 1},
    ],
    "error": [
        {"dx": -1, "flash": True},
        {"dx": 1},
        {"dx": -1, "head": 1},
        {"dx": 0, "head": 1},
    ],
}
STATES = list(POSES)

def heater_b(g,x0,y0,w,h,cross=True):   # original variant-b shield (palette CB)
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
    heater_b(g,8,13,15,15)
    return outline(g)

def to_ans(g,pal=None):
    lines=[]
    pal=pal or C
    def col(k): return pal[k]
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
    "a": (variant_a, "plumed sellsword: blackened great helm with a long blood plume, straight longsword upright in the right hand, battered black heater shield with a worn red cross pattee on the left arm, fur mantle, tattered crimson cloak", None),
    "b": (variant_b, "shield-wall veteran: barbute, large centred heater shield, blade over the shoulder, hooded crimson cloak", "CB"),
}
STOCK = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "sprites", "blackshield-mercenary.ans")


def render(variant):
    fn, desc, pal = VARIANTS[variant]
    pal = CB if pal == "CB" else C
    header = (
        f"# blackshield-mercenary: Black Steel knight, {desc}.\n"
        "# 32x14 cells, truecolor half-blocks.\n"
        "# Palette: docs/brand/tokens.css.\n"
        f"# Generated by scripts/knight-sprite.py (variant {variant}); edit the script, not the art.\n"
    )
    if variant == "a":
        blocks = []
        for state in STATES:
            frames = [knight_a(p) for p in POSES[state]]
            blocks.append(
                f"---state:{state}---\n" + "\n---frame---\n".join("\n".join(to_ans(f, pal)) for f in frames)
            )
        return header + "\n".join(blocks) + "\n"
    frames = [fn(0), fn(1)]
    return header + "\n---frame---\n".join("\n".join(to_ans(f, pal)) for f in frames) + "\n"


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
