#!/usr/bin/env python3
"""Turn the hand-drawn classic track into a spline map. Run once.

The old track was a 256x144 pixel-art sprite -- `classic-reference.png`, kept
here beside this script -- with two rings of wall vertices and a 17-point racing
line hand-clicked over it in `src/track/mod.rs`. The sprite is the only one of
the three that *is* the track: the rings were traced off it by eye, and the
racing line was never meant to be a centreline at all.

So this reads the sprite. The road is the tarmac-coloured region; the centreline
is the middle of that region, and the road's width is how wide that region is.
The racing line survives only as a seed, for ordering and topology -- which way
round the lap goes and where the start line is -- because a pixel region has no
direction.

    ./scripts/convert-classic-track.py > assets/maps/classic.json

The first conversion measured the corridor against the two traced rings instead,
and had to *narrow the road* at nine nodes to stop the rings' noise folding the
corners. Measured against the sprite, the road turns out to be a stroked path of
one constant width the whole way round: 24 units, everywhere. The hairpins are
tighter than that road is wide -- the artwork draws their inner edge as a cusp --
and that is a property of the track, not something to be fixed by pinching it.
"""

import json
import math
import os
import struct
import zlib

UNITS = 256  # map sub-units per world unit, matching MAP_UNITS_PER_WORLD

SPRITE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "classic-reference.png")

# The tarmac, and the start line painted on top of it. Everything else in the
# sprite -- grass, bushes, the lighter tufts -- is off the road.
ROAD_COLOURS = {(50, 51, 83), (255, 255, 255)}

# The direction of travel: +x along the bottom straight, which is the order the
# original list was clicked in and the way the starting grid faces. Only used to
# seed the search; every point of it moves to the middle of the road before
# anything is fitted to it.
PROGRESS = [
    (-16.343735, -46.21621), (32.428055, -46.21621), (57.332794, -16.378376),
    (75.80382, -19.783785), (97.28415, -42.48648), (107.86867, 2.4324331),
    (99.35954, 47.837837), (89.086334, 52.216213), (10.3250885, -15.567566),
    (-72.17187, -17.351353), (-74.76611, 6.162163), (4.825287, 19.945944),
    (12.919342, 42.64865), (-9.079857, 48.486485), (-101.2274, 42.486485),
    (-111.29307, 20.756758), (-104.75557, -47.35135),
]

# The old hardcoded item-spawner positions, in world coordinates.
ITEM_BOXES = [
    (0.0, -52.0), (-0.0, -39.8), (101.0, 3.8), (110.0, 3.4),
    (-81.0, -5.8), (-68.8, -5.7), (-117.2, 21.1), (-106.66667, 20.86859),
]

NODES = 48
SEEDS = 410  # dense samples the medial axis is walked at, about two units apart
HANDLE_REACH = 0.44  # handle length as a fraction of the shorter adjacent span

# A ray that runs *along* the road instead of across it -- which happens at a
# hairpin apex, where there is no meaningful "across" -- reports a corridor
# nothing like the real one. Those samples keep their position for the round.
IMPLAUSIBLE_HALF_WIDTH = 18.0
MAX_STEP = 2.0  # how far a sample may be moved towards the middle in one round
ROUNDS = 40


# --- the sprite -------------------------------------------------------------

def read_png(path):
    """Decode a non-interlaced 8-bit PNG to a grid of RGB tuples.

    Written out rather than imported because this repository has no Python
    dependencies and this script is not worth acquiring one for.
    """
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
    idat, palette, i = b"", None, 8
    width = height = depth = colour_type = None
    while i < len(data):
        size = struct.unpack(">I", data[i:i + 4])[0]
        kind, body = data[i + 4:i + 8], data[i + 8:i + 8 + size]
        if kind == b"IHDR":
            width, height, depth, colour_type, _, _, interlace = struct.unpack(">IIBBBBB", body)
            assert depth == 8 and interlace == 0, "only 8-bit, non-interlaced"
        elif kind == b"PLTE":
            palette = body
        elif kind == b"IDAT":
            idat += body
        i += 12 + size
    raw = zlib.decompress(idat)
    channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[colour_type]
    stride = width * channels
    rows, previous, at = [], bytearray(stride), 0
    for _ in range(height):
        filter_type, at = raw[at], at + 1
        line, at = bytearray(raw[at:at + stride]), at + stride
        for x in range(stride):
            left = line[x - channels] if x >= channels else 0
            up = previous[x]
            upleft = previous[x - channels] if x >= channels else 0
            if filter_type == 1:
                line[x] = (line[x] + left) & 255
            elif filter_type == 2:
                line[x] = (line[x] + up) & 255
            elif filter_type == 3:
                line[x] = (line[x] + ((left + up) >> 1)) & 255
            elif filter_type == 4:
                estimate = left + up - upleft
                da, db, dc = abs(estimate - left), abs(estimate - up), abs(estimate - upleft)
                nearest = left if (da <= db and da <= dc) else (up if db <= dc else upleft)
                line[x] = (line[x] + nearest) & 255
        rows.append(line)
        previous = line
    pixels = []
    for line in rows:
        row = []
        for x in range(width):
            o = x * channels
            if colour_type == 3:
                index = line[o]
                row.append((palette[index * 3], palette[index * 3 + 1], palette[index * 3 + 2]))
            elif colour_type in (2, 6):
                row.append((line[o], line[o + 1], line[o + 2]))
            else:
                row.append((line[o],) * 3)
        pixels.append(row)
    return width, height, pixels


class Road:
    """The sprite as a predicate on world space.

    The sprite was spawned at the origin with no scaling and the game runs at
    256x144, so one pixel is one world unit and the image is centred on (0, 0).
    """

    def __init__(self, path):
        self.width, self.height, pixels = read_png(path)
        self.mask = [[pixel in ROAD_COLOURS for pixel in row] for row in pixels]

    def inside(self, x, y):
        column = int(math.floor(x + self.width / 2))
        row = int(math.floor(self.height / 2 - y))
        if 0 <= column < self.width and 0 <= row < self.height:
            return self.mask[row][column]
        return False

    def edge(self, point, direction, limit=30.0, coarse=0.25):
        """How far from `point` along `direction` the road ends, or None.

        Marched coarsely and then bisected, rather than walked pixel by pixel,
        so the answer is good to about a thousandth of a unit -- the road's
        width is being read off this and a quarter-pixel of slop in it would be
        visible as a wobble in the finished road edge.
        """
        if not self.inside(*point):
            return None
        travelled = 0.0
        while travelled < limit:
            further = travelled + coarse
            if not self.inside(point[0] + direction[0] * further,
                               point[1] + direction[1] * further):
                low, high = travelled, further
                for _ in range(12):
                    middle = (low + high) / 2
                    if self.inside(point[0] + direction[0] * middle,
                                   point[1] + direction[1] * middle):
                        low = middle
                    else:
                        high = middle
                return (low + high) / 2
            travelled = further
        return limit


# --- vectors and closed polylines -------------------------------------------

def sub(a, b):
    return (a[0] - b[0], a[1] - b[1])


def length(v):
    return math.hypot(v[0], v[1])


def normalise(v):
    n = length(v)
    return (v[0] / n, v[1] / n) if n > 1e-9 else (1.0, 0.0)


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
        low, high = 0, n
        while low + 1 < high:
            middle = (low + high) // 2
            if cumulative[middle] <= target:
                low = middle
            else:
                high = middle
        span = cumulative[low + 1] - cumulative[low]
        f = (target - cumulative[low]) / span if span > 1e-9 else 0.0
        a, b = points[low], points[(low + 1) % n]
        out.append((a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f))
    return out, total


def smooth_closed(points, strength):
    """Light averaging around a closed loop, to take the noise off ray-casting."""
    n = len(points)
    return [
        (points[i][0] * (1 - strength) + (points[(i - 1) % n][0] + points[(i + 1) % n][0]) * strength / 2,
         points[i][1] * (1 - strength) + (points[(i - 1) % n][1] + points[(i + 1) % n][1]) * strength / 2)
        for i in range(n)
    ]


def bezier(control, t):
    mt = 1.0 - t
    a, b = mt * mt * mt, 3 * mt * mt * t
    c, d = 3 * mt * t * t, t * t * t
    return (control[0][0] * a + control[1][0] * b + control[2][0] * c + control[3][0] * d,
            control[0][1] * a + control[1][1] * b + control[2][1] * c + control[3][1] * d)


def to_map(p):
    return [int(round(p[0] * UNITS)), int(round(p[1] * UNITS))]


def scalar(v):
    return int(round(v * UNITS))


# --- the medial axis --------------------------------------------------------

road = Road(SPRITE)


def corridor(points, stencil=3):
    """For every point, how far the road reaches either side of it.

    The tangent comes from neighbours three samples away rather than one: at two
    units apart, one sample either side is a six-unit stencil measuring mostly
    the wobble left over from the previous round, and a tangent that wobbles
    casts a normal that wobbles, which is a feedback loop.
    """
    n = len(points)
    out = []
    for i, p in enumerate(points):
        tangent = normalise(sub(points[(i + stencil) % n], points[(i - stencil) % n]))
        normal = (-tangent[1], tangent[0])
        back = (-normal[0], -normal[1])
        out.append((road.edge(p, normal), road.edge(p, back), normal))
    return out


def medial_axis():
    """Walk the seed line into the middle of the road, and measure it there.

    Each round moves every sample halfway between the two edges it can see and
    then re-spaces the loop, which is a fixed-point iteration: the middle of the
    road is the only place a sample stops moving. Steps are capped so a sample
    that starts near the inside wall of a hairpin -- as every point of a racing
    line does -- crosses the road over several rounds instead of overshooting
    into the far wall and dragging its neighbours with it.
    """
    points, _ = resample_closed(PROGRESS, SEEDS)
    for _ in range(ROUNDS):
        moved = []
        for p, (left, right, normal) in zip(points, corridor(points)):
            if left is None or right is None or (left + right) / 2 > IMPLAUSIBLE_HALF_WIDTH:
                moved.append(p)
                continue
            shift = max(-MAX_STEP, min(MAX_STEP, (left - right) / 2))
            moved.append((p[0] + normal[0] * shift, p[1] + normal[1] * shift))
        points, _ = resample_closed(smooth_closed(moved, 0.20), SEEDS)
    measured = [
        (left + right) / 2
        for left, right, _ in corridor(points)
        if left is not None and right is not None and (left + right) / 2 <= IMPLAUSIBLE_HALF_WIDTH
    ]
    return points, measured


centreline, measured = medial_axis()
measured.sort()
half_width = round(measured[len(measured) // 2] * 4) / 4

centres, lap = resample_closed(centreline, NODES)

# Tangents from the *bisector* of the two adjacent directions, with a length
# taken from the spacing.
#
# Plain Catmull-Rom uses `(next - prev) / 6`, which is the chord between the
# neighbours -- and at a hairpin the line doubles back, so that chord is short
# and the handle collapses to a fifth of the local spacing. The curve comes to a
# point. Normalising the two directions first and adding them gives the
# direction the corner actually turns through, and taking the length from the
# spacing keeps it proportionate however sharply it turns.
nodes = []
for i, p in enumerate(centres):
    into = normalise(sub(p, centres[(i - 1) % NODES]))
    out_of = normalise(sub(centres[(i + 1) % NODES], p))
    direction = normalise((into[0] + out_of[0], into[1] + out_of[1]))
    reach = HANDLE_REACH * min(length(sub(p, centres[(i - 1) % NODES])),
                               length(sub(centres[(i + 1) % NODES], p)))
    out = (direction[0] * reach, direction[1] * reach)
    nodes.append({
        "position": to_map(p),
        "in_handle": to_map((-out[0], -out[1])),
        "out_handle": to_map(out),
        # The road is one width the whole way round. Every per-node override the
        # first conversion emitted was an artefact of measuring against the
        # traced rings, and every one of them read on screen as a corner that
        # suddenly went narrow.
        "half_width": None,
        "mirrored": True,
    })


def project(p):
    """World position to the (segment, t, lateral) the format anchors things by."""
    best = None
    for segment in range(NODES):
        a, b = centres[segment], centres[(segment + 1) % NODES]
        ab = sub(b, a)
        denominator = ab[0] ** 2 + ab[1] ** 2
        if denominator < 1e-12:
            continue
        t = max(0.0, min(1.0, ((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / denominator))
        on = (a[0] + t * ab[0], a[1] + t * ab[1])
        d = length(sub(p, on))
        if best is None or d < best[0]:
            tangent = normalise(ab)
            normal = (-tangent[1], tangent[0])
            offset = sub(p, on)
            best = (d, segment, t, offset[0] * normal[0] + offset[1] * normal[1])
    _, segment, t, lateral = best
    # The chord between two nodes cuts inside the curve, so a position measured
    # against it can come out a little past the road edge on a corner. Pull it
    # back on: an item box in the grass is worse than one a metre from where it
    # used to be.
    limit = half_width - 1.5
    lateral = max(-limit, min(limit, lateral))
    return {"segment": segment, "t": min(65535, int(round(t * 65536))),
            "lateral": scalar(lateral)}


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
        "half_width": scalar(half_width),
        "kerb_width": scalar(2.0),
        "kerb_stripe": scalar(9.0),
    },
    "start": {"at": project(PROGRESS[0]), "depth": scalar(4.0), "grid": grid},
    "item_boxes": [project(p) for p in ITEM_BOXES],
    # Small enough that the whole track still fits one 256x144 screen, so
    # `Classic` keeps its static camera and needs no minimap, exactly as it
    # played before it was a spline.
    "bounds_padding": scalar(2.0),
    "decor": {"seed": 20260906, "density": 1.6, "clearance": scalar(3.0)},
}, indent=2))
