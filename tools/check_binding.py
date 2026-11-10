#!/usr/bin/env python3
"""Check that the track and relay blockstates bind every declared state.

Reads the client's built state table (`model-states.json`, produced by a
`--dev-mod content` run; the e2e copies it to
`/tmp/opencode/redstonepen-model-states.json`) and the content's blockstate
JSONs. A state with no matching selector is "unbound"; a selector naming a
property the block does not have is an error.
"""
import json
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CONTENT = ROOT / "content"
DEFAULT_STATES = Path("/tmp/opencode/redstonepen-model-states.json")
BLOCKS = [
    "redstonepen:track",
    "redstonepen:relay",
    "redstonepen:inverted_relay",
    "redstonepen:pulse_relay",
    "redstonepen:bistable_relay",
]


def key_matches(selector, state):
    if not selector:
        return True
    for clause in selector.split(","):
        name, _, value = clause.partition("=")
        if state.get(name) != value:
            return False
    return True


def check_selector_names(selector, state, block, where):
    names = set()
    if selector:
        for clause in selector.split(","):
            names.add(clause.partition("=")[0])
    for name in names:
        if name not in state:
            raise SystemExit(f"{block}: {where} selects unknown property {name}")


def main():
    states_path = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_STATES
    table = json.loads(states_path.read_text())
    failed = False
    for block in BLOCKS:
        entry = table.get(block)
        if entry is None:
            raise SystemExit(f"{block}: not in {states_path}")
        states = entry["states"]
        blockstate = json.loads(
            (CONTENT / "assets/redstonepen/blockstates" / f"{block.split(':')[1]}.json").read_text()
        )
        variants = blockstate["variants"]
        for selector in variants:
            check_selector_names(selector, states[0], block, "variant")
        counts = [sum(1 for selector in variants if key_matches(selector, state)) for state in states]
        unbound = sum(1 for count in counts if count == 0)
        print(
            f"{block}: {len(states)} states, {len(variants)} variants, "
            f"{unbound} unbound, variant counts {dict(Counter(counts))}"
        )
        failed |= unbound != 0 or any(count != 1 for count in counts)
    print("FAIL: a state did not bind" if failed else "PASS: every state binds")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
