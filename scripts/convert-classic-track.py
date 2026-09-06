#!/usr/bin/env python3
"""Turn the hand-traced classic track into a spline map. Run once.

The old track was three hand-clicked lists of points in `src/track/mod.rs`: a
17-point racing line, and two rings of wall vertices traced by eye against a
256x144 pixel-art sprite. This reads those, fits a closed bezier loop through the
racing line, and measures the road's width at each node from the two rings.

The result is deliberately about 80% right. It is a starting point a human
finishes in the level editor, not a faithful reproduction -- the racing line cuts
corners, because it was a racing line.

    ./scripts/convert-classic-track.py > assets/maps/classic.json
"""

import json
import math

UNITS = 256  # map sub-units per world unit, matching MAP_UNITS_PER_WORLD

# The direction of travel: +x along the bottom straight, which is the order the
# original list was clicked in and the way the starting grid faces.
PROGRESS = [
    (-16.343735, -46.21621), (32.428055, -46.21621), (57.332794, -16.378376),
    (75.80382, -19.783785), (97.28415, -42.48648), (107.86867, 2.4324331),
    (99.35954, 47.837837), (89.086334, 52.216213), (10.3250885, -15.567566),
    (-72.17187, -17.351353), (-74.76611, 6.162163), (4.825287, 19.945944),
    (12.919342, 42.64865), (-9.079857, 48.486485), (-101.2274, 42.486485),
    (-111.29307, 20.756758), (-104.75557, -47.35135),
]

OUTER = [
    (-97.0, -61.5), (33.0, -57.2), (48.2, -47.4), (55.7, -38.0), (61.6, -26.0),
    (66.0, -25.6), (76.6, -45.8), (86.0, -54.0), (99.8, -54.2), (106.5, -51.4),
    (114.6, -43.4), (119.2, -27.0), (119.6, 4.2), (115.4, 48.0), (110.6, 57.4),
    (100.2, 63.2), (87.8, 63.6), (69.3, 53.0), (53.4, 41.6), (14.0, 0.0),
    (7.0, -1.4), (0.1, -5.5), (-54.2, -10.6), (-63.0, -6.5), (-59.6, -0.2),
    (-35.1, 0.2), (-9.6, 2.4), (9.8, 11.0), (23.2, 22.6), (27.0, 31.2),
    (27.0, 39.0), (13.8, 54.6), (-10.0, 60.0), (-47.2, 58.2), (-90.0, 57.8),
    (-106.6, 50.0), (-119.6, 37.2), (-124.0, 26.2), (-123.8, -34.8),
    (-120.6, -45.6), (-109.0, -57.8),
]

INNER = [
    (-92.0, -37.8), (-50.6, -35.2), (15.8, -33.2), (31.8, -32.0), (41.8, -13.8),
    (54.4, -1.4), (72.4, -1.6), (85.4, -11.8), (94.5, -29.0), (95.4, -1.4),
    (92.4, 20.6), (93.6, 36.4), (89.4, 40.0), (83.0, 33.8), (62.6, 17.8),
    (26.2, -21.0), (5.6, -29.8), (-20.2, -31.2), (-77.6, -30.8), (-84.7, -25.8),
    (-90.2, -18.8), (-90.4, 5.0), (-84.6, 14.8), (-69.8, 23.6), (-26.1, 25.2),
    (-13.0, 28.0), (-0.8, 31.4), (-0.2, 34.8), (-27.4, 35.6), (-87.2, 33.0),
    (-98.6, 25.8), (-100.6, -22.2), (-98.4, -35.2),
]

# The old hardcoded item-spawner positions, in world coordinates.
ITEM_BOXES = [
    (0.0, -52.0), (-0.0, -39.8), (101.0, 3.8), (110.0, 3.4),
    (-81.0, -5.8), (-68.8, -5.7), (-117.2, 21.1), (-106.66667, 20.86859),
]

NODES = 24
CLEARANCE = 1.0  # pulled in from the traced wall, so the road never touches it


def sub(a, b):
    return (a[0] - b[0], a[1] - b[1])


def length(v):
    return math.hypot(v[0], v[1])


def point_to_segment(p, a, b):
    ab = sub(b, a)
    denom = ab[0] ** 2 + ab[1] ** 2
    if denom < 1e-12:
        return length(sub(p, a))
    t = max(0.0, min(1.0, ((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / denom))
    return length(sub(p, (a[0] + t * ab[0], a[1] + t * ab[1])))


def clearance_to_ring(p, ring):
    """Perpendicular distance to the nearest wall segment.

    Perpendicular rather than a ray cast along the road's normal: the outer ring
    has a peninsula pushed into the middle of the track and the inner ring is one
    self-touching loop around two infield islands, so a ray finds the wrong wall
    exactly where the geometry is interesting. The nearest wall is always the one
    that constrains the road.
    """
    return min(point_to_segment(p, ring[i], ring[(i + 1) % len(ring)])
               for i in range(len(ring)))


def resample_closed(points, count):
    """Even spacing by arc length around a closed polyline."""
    n = len(points)
    cumulative = [0.0]
    for i in range(n):
        cumulative.append(cumulative[-1] + length(sub(points[(i + 1) % n], points[i])))
    total = cumulative[-1]
    out = []
    for k in range(count):
        target = total * k / count
        i = max(j for j in range(n + 1) if cumulative[j] <= target)
        i = min(i, n - 1)
        span = cumulative[i + 1] - cumulative[i]
        f = (target - cumulative[i]) / span if span > 1e-9 else 0.0
        a, b = points[i], points[(i + 1) % n]
        out.append((a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f))
    return out, total


def to_map(p):
    return [int(round(p[0] * UNITS)), int(round(p[1] * UNITS))]


def scalar(v):
    return int(round(v * UNITS))


centres, lap = resample_closed(PROGRESS, NODES)

# Cyclic Catmull-Rom as bezier handles: out_i = (p[i+1] - p[i-1]) / 6. That is
# the exact bezier equivalent, so the curve passes through every seeded point and
# is C1 -- and the handles come out mirrored, which is the right starting state
# for an author who can then break the mirror.
nodes = []
widths = []
for i, p in enumerate(centres):
    prev = centres[(i - 1) % NODES]
    nxt = centres[(i + 1) % NODES]
    out = ((nxt[0] - prev[0]) / 6.0, (nxt[1] - prev[1]) / 6.0)
    widths.append(max(2.0, min(clearance_to_ring(p, OUTER),
                               clearance_to_ring(p, INNER)) - CLEARANCE))
    nodes.append({
        "position": to_map(p),
        "in_handle": to_map((-out[0], -out[1])),
        "out_handle": to_map(out),
        "half_width": None,
        "mirrored": True,
    })

# One base width for the map, with an explicit override only where the traced
# track is genuinely wider or narrower than that. Per-node rather than a single
# average because the hand-drawn track is not uniform, and averaging it away is
# what a conversion is supposed to avoid.
ordered = sorted(widths)
base = ordered[len(ordered) // 2]
for node, w in zip(nodes, widths):
    if abs(w - base) > 1.0:
        node["half_width"] = scalar(w)


def project(p):
    """World position to the (segment, t, lateral) the format anchors things by."""
    best = None
    for seg in range(NODES):
        a = centres[seg]
        b = centres[(seg + 1) % NODES]
        ab = sub(b, a)
        denom = ab[0] ** 2 + ab[1] ** 2
        if denom < 1e-12:
            continue
        t = max(0.0, min(1.0, ((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / denom))
        on = (a[0] + t * ab[0], a[1] + t * ab[1])
        d = length(sub(p, on))
        if best is None or d < best[0]:
            tangent = (ab[0] / math.sqrt(denom), ab[1] / math.sqrt(denom))
            normal = (-tangent[1], tangent[0])  # left of travel
            offset = sub(p, on)
            lateral = offset[0] * normal[0] + offset[1] * normal[1]
            best = (d, seg, t, lateral)
    _, seg, t, lateral = best
    # The racing line cuts corners, so a position measured against it can land
    # past the road edge. Pull it back on: an item box in the grass is worse than
    # one a metre from where it used to be.
    limit = max(1.0, min(widths[seg], widths[(seg + 1) % NODES]) - 1.5)
    lateral = max(-limit, min(limit, lateral))
    return {"segment": seg, "t": min(65535, int(round(t * 65536))),
            "lateral": scalar(lateral)}


start = project(PROGRESS[0])

# Read straight off the old formula: `(-25 + (i / 3) * -10, -39 + (i % 3) * -7)`
# against a start line at x = -16.34.
grid = {
    "columns": 3,
    "row_spacing": scalar(10.0),
    "column_spacing": scalar(7.0),
    "first_row_offset": scalar(-16.343735 - (-25.0)),
}

print(json.dumps({
    "version": 1,
    "name": "Classic",
    "nodes": nodes,
    "road": {
        "half_width": scalar(base),
        "kerb_width": scalar(1.5),
        "kerb_stripe": scalar(9.0),
    },
    "start": {"at": start, "depth": scalar(4.0), "grid": grid},
    "item_boxes": [project(p) for p in ITEM_BOXES],
    "bounds_padding": scalar(16.0),
    "decor": {"seed": 20260906, "density": 1.6, "clearance": scalar(3.0)},
}, indent=2))
