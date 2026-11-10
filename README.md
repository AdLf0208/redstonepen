# redstonepen (Horizon port)

A Horizon port of [redstonepen](stfwi/redstonepen) — MIT.

**Status: first slices.** What is below is what the port actually does today.

## Implemented so far

- | Behaviour | Upstream |
- |---|---|
- | Track registration (`noCollission().instabreak().dynamicShape().randomTicks()`) | `ModContent.java:39-45` |
- | Pen/quill registration (stack 1; pen durability 256) | `ModContent.java:127-132` |
- | Relay registration | `ModContent.java:59-66` |
- | 24 wire bits, 6 connectors, 6 face powers | `RedstoneTrack.java:71-78` |
- | Segment ↔ face/direction map, internal edges | `RedstoneTrack.java:117-158` |
- | Internal-edge segments are one net with one power on both internal sides | `RedstoneTrack.java:1146-1312` (edges `:1165-1180`, net `:1219-1312`) |

## Not yet ported

- One track per block, two or three arms** (the brief's documented
- simplification): no multiple tracks/connectors in one cell, no per-face
- attachment, and no fourth arm (a cross/4-way junction is refused). Upstream's
- segment block entity is not expressible in states; a corner is the
- two-arm net that fits, a T the three-arm net (1584 states).
- Corner/T collision box:** the state's `axis` follows the first arm and the
- content `shapes` bar covers that arm only; the model shows all arms. Content
- `shapes` can only be keyed on a template property, so a per-shape collision

## Install

Build the bundle with a native-mod capable launcher, or add the file from the
registry once it is listed. Pinned to Horizon `1d63e0a` and horizon-mixins
`5a64af93` for game 26.3.

## Build from source

```sh
cargo test
python3 tools/horizon.py check --horizon <horizon checkout>
python3 tools/horizon.py build --horizon <horizon checkout>
python3 tools/package.py
```

## Credits and licence

Original mod: stfwi/redstonepen. Port: AdLf0208. Licence: MIT — see [LICENSE](LICENSE);
the original notices are preserved.
