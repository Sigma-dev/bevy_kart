#!/usr/bin/env python3
"""Generate `Sweeping Bends`, the second built-in map. Run once.

Deliberately larger than one screen -- about two by one and a half -- because
that is the case the follow camera and the minimap exist for, and a map that
fits on one screen exercises neither.

The shape is a radius that varies with angle by a couple of harmonics, which
gives a closed loop with a natural mix of long curves and tighter ones without
any of it being hand-placed. The road then narrows through the tight parts and
opens out on the fast ones, which is the difference between a circuit and a
loop.

    ./scripts/make-sweeping-map.py > assets/maps/sweeping.json
"""

import json
import math

UNITS = 256
NODES = 22
RADIUS_X = 250.0
RADIUS_Y = 150.0
WIDE = 13.0     # half-width on the fast sweeps
NARROW = 7.5    # half-width through the tight parts
FOLD_LIMIT = 0.8
HANDLE_REACH = 0.38


def shape(theta):
    """A wobbled ellipse: two harmonics, chosen to avoid a lap that doubles back
    close to itself, which nearest-segment progress cannot tell apart."""
    r = 1.0 + 0.16 * math.sin(3.0 * theta + 0.6) + 0.09 * math.sin(2.0 * theta - 1.1)
    return (RADIUS_X * r * math.cos(theta), RADIUS_Y * r * math.sin(theta))


def sub(a, b):
    return (a[0] - b[0], a[1] - b[1])


def length(v):
    return math.hypot(v[0], v[1])


def curvature(prev, p, nxt):
    a, b = sub(p, prev), sub(nxt, p)
    cross = a[0] * b[1] - a[1] * b[0]
    denom = length(a) * length(b) * length(sub(nxt, prev))
    return 2.0 * cross / denom if denom > 1e-9 else 0.0


def to_map(p):
    return [int(round(p[0] * UNITS)), int(round(p[1] * UNITS))]


def scalar(v):
    return int(round(v * UNITS))


# Even spacing in arc length rather than in angle, so the nodes are not bunched
# at the ends of the ellipse.
dense = [shape(2.0 * math.pi * i / 2000) for i in range(2000)]
cumulative = [0.0]
for i in range(len(dense)):
    cumulative.append(cumulative[-1] + length(sub(dense[(i + 1) % len(dense)], dense[i])))
total = cumulative[-1]

centres = []
for k in range(NODES):
    target = total * k / NODES
    i = max(j for j in range(len(dense)) if cumulative[j] <= target)
    span = cumulative[i + 1] - cumulative[i]
    f = (target - cumulative[i]) / span if span > 1e-9 else 0.0
    a, b = dense[i], dense[(i + 1) % len(dense)]
    centres.append((a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f))

nodes, widths = [], []
for i, p in enumerate(centres):
    prev, nxt = centres[(i - 1) % NODES], centres[(i + 1) % NODES]
    out = ((nxt[0] - prev[0]) / 6.0, (nxt[1] - prev[1]) / 6.0)
    reach = HANDLE_REACH * min(length(sub(p, prev)), length(sub(nxt, p)))
    if length(out) > reach > 0:
        scale = reach / length(out)
        out = (out[0] * scale, out[1] * scale)
    # Wide where the track is straight, narrow where it turns: the road tells you
    # what is coming before you get there.
    k = abs(curvature(prev, p, nxt))
    tightness = min(1.0, k * 90.0)
    w = WIDE + (NARROW - WIDE) * tightness
    if k > 1e-6:
        w = min(w, FOLD_LIMIT / k)
    widths.append(max(NARROW * 0.8, w))
    nodes.append({
        "position": to_map(p),
        "in_handle": to_map((-out[0], -out[1])),
        "out_handle": to_map(out),
        "half_width": None,
        "mirrored": True,
    })

ordered = sorted(widths)
base = ordered[len(ordered) // 2]
for node, w in zip(nodes, widths):
    if abs(w - base) > 0.5:
        node["half_width"] = scalar(w)

# Item boxes in pairs, spread around the lap and offset either side of centre.
item_boxes = []
for k in range(6):
    segment = int(NODES * k / 6)
    for lateral in (-4.0, 4.0):
        item_boxes.append({"segment": segment, "t": 32768, "lateral": scalar(lateral)})

print(json.dumps({
    "version": 1,
    "name": "Sweeping Bends",
    "nodes": nodes,
    "road": {"half_width": scalar(base), "kerb_width": scalar(1.5),
             "kerb_stripe": scalar(10.0)},
    "start": {
        "at": {"segment": 0, "t": 0, "lateral": 0},
        "depth": scalar(4.0),
        "grid": {"columns": 3, "row_spacing": scalar(11.0),
                 "column_spacing": scalar(7.0), "first_row_offset": scalar(9.0)},
    },
    "item_boxes": item_boxes,
    "bounds_padding": scalar(20.0),
    "decor": {"seed": 4242, "density": 1.4, "clearance": scalar(3.0)},
}, indent=2))
