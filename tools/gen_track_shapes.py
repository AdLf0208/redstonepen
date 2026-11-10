#!/usr/bin/env python3
"""Regenerate the track's outline and collision boxes from its bend arms.

The track state is `axis` x `bend` x `power`. `axis` is a template property
(basalt) and `bend` is declared, so the shape table is keyed on the ordered
tuple `["bend", "axis"]` (99 entries) and each corner/T follows its own arms.
A straight track (`bend = none`) follows `axis`; the 12 corners and 20 Ts are
axis-independent, so their three entries repeat.

Run from the port root after editing the bend list:

    python3 tools/gen_track_shapes.py

It rewrites `content/data/redstonepen/horizon/block/track.json` in place and
leaves every other key untouched.
"""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PATH = ROOT / "content/data/redstonepen/horizon/block/track.json"

# 2-pixel bar, centred in the cell.
LO, HI, MID = 0.4375, 0.5625, 0.5

# A half-bar from the centre to each face, as [minX, minY, minZ, maxX, maxY, maxZ].
ARM = {
    "down": [LO, 0.0, LO, HI, MID, HI],
    "up": [LO, MID, LO, HI, 1.0, HI],
    "north": [LO, LO, 0.0, HI, HI, MID],
    "south": [LO, LO, MID, HI, HI, 1.0],
    "west": [0.0, LO, LO, MID, HI, HI],
    "east": [MID, LO, LO, 1.0, HI, HI],
}

# A straight track is the two opposite arms of its axis: one full bar.
STRAIGHT = {
    "x": [[0.0, LO, LO, 1.0, HI, HI]],
    "y": [[LO, 0.0, LO, HI, 1.0, HI]],
    "z": [[LO, LO, 0.0, HI, HI, 1.0]],
}


def boxes(bend, axis):
    if bend == "none":
        return STRAIGHT[axis]
    return [ARM[arm] for arm in bend.split("_")]


def shapes(values):
    # Every (bend, axis) combination, in a stable order.
    return {
        "properties": ["bend", "axis"],
        "values": {
            f"{bend},{axis}": boxes(bend, axis)
            for bend in values
            for axis in ("x", "y", "z")
        },
    }


def main():
    track = json.loads(PATH.read_text())
    table = shapes(track["state_properties"]["bend"]["values"])
    ordered = {
        "template": track["template"],
        "properties": track["properties"],
        "shapes": table,
        "collision_shapes": table,
        "state_properties": track["state_properties"],
        "default_state": track["default_state"],
    }
    PATH.write_text(json.dumps(ordered, indent=2) + "\n")
    print(f"{PATH}: {len(table['values'])} (bend, axis) shape entries")


if __name__ == "__main__":
    main()
