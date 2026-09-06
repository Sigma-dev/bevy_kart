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

NODES = 44
CLEARANCE = 1.0  # pulled in from the traced wall, so the road never touches it
FOLD_LIMIT = 0.8  # how close to folding the inner edge is allowed to get
HANDLE_REACH = 0.36  # handle length as a fraction of the shorter adjacent span
MIN_PLAYABLE_HALF_WIDTH = 5.0  # a road ten units across: two and a half karts


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


def ray_hit(origin, direction, ring):
    """Nearest crossing of a ray with a closed polyline, or None.

    Both rings together are the corridor's walls. Casting perpendicular from a
    point inside it hits the outer wall one way and the inner wall the other, and
    "nearest hit in this direction" picks the right one without anybody having to
    decide which ring is which -- which matters because the outer ring has a
    peninsula pushed into the middle of the track and the inner one is a single
    self-touching loop around two islands, so "outer" and "inner" stop meaning
    "further" and "nearer" exactly where the track is interesting.
    """
    best = None
    for i in range(len(ring)):
        a, b = ring[i], ring[(i + 1) % len(ring)]
        seg = sub(b, a)
        denom = direction[0] * seg[1] - direction[1] * seg[0]
        if abs(denom) < 1e-9:
            continue
        diff = sub(a, origin)
        t = (diff[0] * seg[1] - diff[1] * seg[0]) / denom     # along the ray
        u = (diff[0] * direction[1] - diff[1] * direction[0]) / denom  # along the segment
        if t > 1e-6 and -1e-9 <= u <= 1.0 + 1e-9 and (best is None or t < best):
            best = t
    return best


def corridor_at(point, direction):
    """Centre and half-width of the road across `point`, or None if it is unclear.

    The seed line is a *racing* line: it cuts corners, so it runs near the inside
    wall through every turn. Measuring the corridor and stepping to the middle of
    it is what turns it back into a centreline -- and gives the true width for
    free, which is the same measurement.
    """
    normal = (-direction[1], direction[0])
    walls = OUTER + [OUTER[0]], INNER + [INNER[0]]
    left = min((h for h in (ray_hit(point, normal, OUTER), ray_hit(point, normal, INNER))
                if h is not None), default=None)
    back = (-normal[0], -normal[1])
    right = min((h for h in (ray_hit(point, back, OUTER), ray_hit(point, back, INNER))
                 if h is not None), default=None)
    if left is None or right is None:
        return None
    # Shifted to the middle of what the rays found.
    shift = (left - right) / 2.0
    centre = (point[0] + normal[0] * shift, point[1] + normal[1] * shift)
    return centre, (left + right) / 2.0


def smooth_closed(points, rounds, strength=0.5):
    """Light averaging around a closed loop, to take the noise off ray-casting."""
    for _ in range(rounds):
        n = len(points)
        points = [
            (
                points[i][0] * (1 - strength)
                + (points[(i - 1) % n][0] + points[(i + 1) % n][0]) * strength / 2,
                points[i][1] * (1 - strength)
                + (points[(i - 1) % n][1] + points[(i + 1) % n][1]) * strength / 2,
            )
            for i in range(n)
        ]
    return points


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
        i = max(j for j in range(n) if cumulative[j] <= target)
        span = cumulative[i + 1] - cumulative[i]
        f = (target - cumulative[i]) / span if span > 1e-9 else 0.0
        a, b = points[i], points[(i + 1) % n]
        out.append((a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f))
    return out, total


def sample_at(values, points, query):
    """Nearest value from a parallel list, by position."""
    best, best_d = values[0], None
    for value, point in zip(values, points):
        d = length(sub(point, query))
        if best_d is None or d < best_d:
            best, best_d = value, d
    return best


def curvature(prev, p, nxt):
    a, b = sub(p, prev), sub(nxt, p)
    cross = a[0] * b[1] - a[1] * b[0]
    denom = length(a) * length(b) * length(sub(nxt, prev))
    return 2.0 * cross / denom if denom > 1e-9 else 0.0


def to_map(p):
    return [int(round(p[0] * UNITS)), int(round(p[1] * UNITS))]


def scalar(v):
    return int(round(v * UNITS))


# 1. A dense seed loop, for ordering and topology only.
seed, _ = resample_closed(PROGRESS, 400)

# 2. Step each seed point to the middle of the corridor it is in, and measure how
#    wide that corridor is. This is what recovers the *track*, rather than a
#    racing line inflated into a road.
centred, measured = [], []
for i, p in enumerate(seed):
    prev, nxt = seed[(i - 1) % len(seed)], seed[(i + 1) % len(seed)]
    tangent = sub(nxt, prev)
    span = length(tangent)
    if span < 1e-9:
        continue
    tangent = (tangent[0] / span, tangent[1] / span)
    found = corridor_at(p, tangent)
    if found is None:
        continue
    centre, half = found
    # A ray that escapes through a gap between rings reports an absurd corridor;
    # the real one is nowhere near forty units across.
    if half > 24.0 or half < 2.0:
        continue
    centred.append(centre)
    measured.append(half)

centred = smooth_closed(centred, 6)
centres, lap = resample_closed(centred, NODES)
widths = [max(3.0, sample_at(measured, centred, c) - CLEARANCE) for c in centres]

# 3. Tangents from the *bisector* of the two adjacent directions, with a length
#    taken from the spacing.
#
#    Plain Catmull-Rom uses `(next - prev) / 6`, which is the chord between the
#    neighbours -- and at a hairpin the line doubles back, so that chord is short
#    and the handle collapses to a fifth of the local spacing. The curve comes to
#    a point. Measured on this track, six nodes had handles under a third of
#    their spacing and every one of them was a corner that should have been round.
#
#    Normalising the two directions first and adding them gives the direction the
#    corner actually turns through, and taking the length from the spacing keeps
#    it proportionate however sharply it turns.
nodes = []
for i, p in enumerate(centres):
    prev, nxt = centres[(i - 1) % NODES], centres[(i + 1) % NODES]
    into, out_of = sub(p, prev), sub(nxt, p)
    la, lb = length(into), length(out_of)
    if la < 1e-9 or lb < 1e-9:
        direction = (1.0, 0.0)
    else:
        direction = (into[0] / la + out_of[0] / lb, into[1] / la + out_of[1] / lb)
        d = length(direction)
        direction = (direction[0] / d, direction[1] / d) if d > 1e-9 else (out_of[0] / lb, out_of[1] / lb)
    reach = HANDLE_REACH * min(la, lb)
    out = (direction[0] * reach, direction[1] * reach)
    nodes.append({
        "position": to_map(p),
        "in_handle": to_map((-out[0], -out[1])),
        "out_handle": to_map(out),
        "half_width": None,
        "mirrored": True,
    })


def bezier(control, t):
    mt = 1.0 - t
    a, b = mt * mt * mt, 3 * mt * mt * t
    c, d = 3 * mt * t * t, t * t * t
    return (control[0][0] * a + control[1][0] * b + control[2][0] * c + control[3][0] * d,
            control[0][1] * a + control[1][1] * b + control[2][1] * c + control[3][1] * d)


def pinch_to_fit_the_corners(nodes, widths, stencil=2.0):
    """Cap each node's width by the tightest corner it is responsible for.

    A road cannot be wider than the corner it goes round: past `w * |k| = 1` its
    inner edge folds through itself, which is what `TrackWarning::CornerTooTight`
    reports. Curvature is measured across two world units, the same scale the
    builder works at -- three points a fraction of a unit apart measure the last
    bits of the float rather than the shape of the corner, and report radii of two
    or three units on corners that are perfectly drivable.
    """
    caps = [float("inf")] * len(nodes)
    for seg in range(len(nodes)):
        a, b = nodes[seg], nodes[(seg + 1) % len(nodes)]
        control = [
            (a["position"][0] / UNITS, a["position"][1] / UNITS),
            ((a["position"][0] + a["out_handle"][0]) / UNITS,
             (a["position"][1] + a["out_handle"][1]) / UNITS),
            ((b["position"][0] + b["in_handle"][0]) / UNITS,
             (b["position"][1] + b["in_handle"][1]) / UNITS),
            (b["position"][0] / UNITS, b["position"][1] / UNITS),
        ]
        span = length(sub(control[3], control[0])) + length(sub(control[1], control[0]))
        steps = max(8, int(span / (stencil * 0.5)))
        pts = [bezier(control, i / steps) for i in range(steps + 1)]
        reach = max(1, int(round(stencil / max(span / steps, 1e-6))))
        for i in range(reach, steps - reach):
            k = abs(curvature(pts[i - reach], pts[i], pts[i + reach]))
            if k < 1e-6:
                continue
            cap = FOLD_LIMIT / k
            target = seg if i < steps / 2 else (seg + 1) % len(nodes)
            caps[target] = min(caps[target], cap)
    return [max(MIN_PLAYABLE_HALF_WIDTH, min(w, cap)) for w, cap in zip(widths, caps)]


widths = pinch_to_fit_the_corners(nodes, widths)

ordered = sorted(widths)
base = ordered[len(ordered) // 2]
for node, w in zip(nodes, widths):
    if abs(w - base) > 0.6:
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
            normal = (-tangent[1], tangent[0])
            offset = sub(p, on)
            best = (d, seg, t, offset[0] * normal[0] + offset[1] * normal[1])
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
    # Small enough that the whole track still fits one 256x144 screen, so
    # `Classic` keeps its static camera and needs no minimap, exactly as it
    # played before it was a spline.
    "bounds_padding": scalar(6.0),
    "decor": {"seed": 20260906, "density": 1.6, "clearance": scalar(3.0)},
}, indent=2))
