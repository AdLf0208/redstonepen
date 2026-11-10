#!/usr/bin/env bash
# In-game check for the Redstone Pen track slice (straight and corner tracks).
#
# The End, creative, headless, the content folder loaded with --dev-mod. Rigs:
#
# Default run: the game loads `content/` as a dev mod.
# EMBEDDED=1: the game is launched with **only the native bundle** (no
# `--dev-mod`) and the content comes from the bundle's embedded copy
# (`mixins/<id>/data/…`, the AE2 pattern). The run also checks the file the
# helper wrote, and CONTROL=1 (default with EMBEDDED) first launches the same
# binary with `.horizon-cache` unusable, where the block and item ids must be
# unknown.
#  - a 5-track line from a redstone block lights its far lamp; the power reads
#    15,14,13,12,11; cutting the middle track darkens the lamp and drains the
#    far side; restoring the track lights it again
#  - a 20-track line leaves the far lamp dark (decay): track 5 is 11, track 15
#    is 1, track 16 and the far end are 0
#  - a relay after ten decayed tracks re-strengthens to 15: tracks 12..20 read
#    15..7 and the far lamp is lit; replacing the relay with a plain track
#    drops the far end to 0 and darkens the lamp (counterfactual)
#  - a stone block between the source and the lamp emits nothing; a track line
#    with no source emits nothing
#  - an unpowered inverted relay lights its lamp and a powered one darkens it;
#    the same rig with a plain relay behaves the other way (differential)
#  - a pulse relay emits a 2-tick pulse on the rising input edge and clears
#    while a plain relay holds; a bistable relay latches it and toggles again on
#    the next rising edge
#  - a bistable relay toggles on a rising edge, holds its lamp when the input
#    falls, and toggles back on the next rising edge
#  - a corner track turns the line inside one block: a source, a straight
#    track, a corner (west + north) and a straight track put 15, 14, 13 on
#    the blocks and light the lamp on the far arm; a straight track at the
#    turn (the cross-axis control) leaves the far arm dark, a corner with the
#    wrong arms is dark, cutting the corner darkens it and restoring it lights
#    it again; a straight two-track line is lit with 15, 14 (same decay)
#  - a T junction (west, east and south arms) carries one power to both derive
#    arms: a source on the west puts 15, 14, 13, 13 and lights both lamps;
#    cutting either arm leaves the other side lit; a third arm that is missing
#    the fed side, one with the wrong third arm, and a plain straight track at
#    the junction all leave the right side dark
#  - a pen click draws a straight track, a click on its end extends it, a
#    click on the side of a wired track bends it into a corner, a side click
#    on a track whose two arms are both wired widens it into a T, and a click
#    on the new arm extends that; the drawn L carries the same power
#  - a sneak click is the erase modifier: it drops the clicked arm of a T (or
#    removes a wired track cell) without cutting its neighbours first, while a
#    plain click still bends; a sneak click on a support draws nothing
#  - a pen click on a support block draws a track on the clicked face, a click
#    on its end extends it, a click across it erases it
#  - all 48 straight track states, all 192 corner states, all 320 T states and
#    72 relay states (12 plain + 12 inverted + 24 pulse + 24 bistable) are
#    accepted; an undeclared property and a bad value are rejected
#
#   tools/e2e.sh                     # the pen
#   COUNTERFACTUAL=1 tools/e2e.sh    # the same click with minecraft:stick:
#                                    #   no track is drawn (proves the pen item
#                                    #   is what draws; the signal rigs still
#                                    #   run because they use setblock)
#   SELFTEST=1 tools/e2e.sh          # corrupts the 20-line expectation on
#                                    #   purpose: the run must fail
#   SELFTEST_RELAY=1 tools/e2e.sh    # corrupts the inverted-relay expectation
#                                    #   on purpose: the run must fail at it
#   SELFTEST_CORNER=1 tools/e2e.sh   # corrupts the corner-lamp expectation
#                                    #   on purpose: the run must fail at it
#   SELFTEST_T=1 tools/e2e.sh        # corrupts the T-lamp expectation on
#                                    #   purpose: the run must fail at it
#   SELFTEST_ERASE=1 tools/e2e.sh    # corrupts the sneak-erase expectation on
#                                    #   purpose: the run must fail at it
set -euo pipefail

CLONE=${CAMPAIGN_ROOT:-$(cd "$(dirname "$0")/../../.." && pwd)}
PORT=$CLONE/ports/redstonepen
# The control binary is shared at the campaign pin; the game is this port's build.
CTL=${CTL:-$CLONE/ports/_shared/bin/horizon-1d63e0a5/horizon-ctl}
GAME=${GAME:-$PORT/target/horizon-cargo/fast/horizon-client}
RES=${RES:-${HORIZON_RES:-${HORIZON_RES:-$HOME/.local/horizon-client/26.3}}}
SOCK=${SOCK:-/tmp/opencode/redstonepen-e2e.sock}
COUNTERFACTUAL=${COUNTERFACTUAL:-0}
SELFTEST=${SELFTEST:-0}
SELFTEST_RELAY=${SELFTEST_RELAY:-0}
SELFTEST_CORNER=${SELFTEST_CORNER:-0}
SELFTEST_T=${SELFTEST_T:-0}
SELFTEST_ERASE=${SELFTEST_ERASE:-0}
EMBEDDED=${EMBEDDED:-0}
CONTROL=${CONTROL:-$EMBEDDED}
WORLD=RedstonePenE2E
CONTROL_WORLD=RedstonePenControl

[ -x "$CTL" ] || { echo "missing horizon-ctl: $CTL" >&2; exit 1; }
[ -x "$GAME" ] || { echo "missing horizon-client: $GAME" >&2; exit 1; }
[ -d "$RES/assets" ] || { echo "missing prepared resources: $RES" >&2; exit 1; }
[ -f "$PORT/content/horizon-mod.toml" ] || { echo "missing content mod: $PORT/content" >&2; exit 1; }

if [ "$COUNTERFACTUAL" = 1 ]; then
    TOOL=minecraft:stick
else
    TOOL=redstonepen:pen
fi

SCRATCH=$(mktemp -d /tmp/opencode/redstonepen-home.XXXXXX)
export HORIZON_HOME=$SCRATCH/game
export HORIZON_LAUNCHER_HOME=$SCRATCH/launcher
export XDG_DATA_HOME=$SCRATCH/data
export XDG_CONFIG_HOME=$SCRATCH/config
export HORIZON_NO_AUDIO=1
mkdir -p "$(dirname "$SOCK")"
rm -f "$SOCK"

ctl() { "$CTL" --socket "$SOCK" "$@"; }
fail() { echo "FAIL: $*" >&2; exit 1; }

# `setblock` (with neighbour updates) and insist the state was accepted.
setblk() { # x y z state
    local reply
    if ! reply=$(ctl setblock "$1" "$2" "$3" "$4" 2>&1); then
        fail "setblock $4 at $1,$2,$3 was rejected: $reply"
    fi
    printf '%s' "$reply" | jq -e '.ok == true' >/dev/null \
        || fail "setblock $4 at $1,$2,$3 was rejected: $reply"
}
fill() { # x1 y1 z1 x2 y2 z2 state
    local reply
    reply=$(ctl fill "$1" "$2" "$3" "$4" "$5" "$6" "$7")
    printf '%s' "$reply" | jq -e '.ok == true' >/dev/null || fail "fill $7: $reply"
}
state_at() { ctl block "$1" "$2" "$3" | jq -r '.state // empty'; }
expect_state_has() { # x y z substring
    local state
    state=$(state_at "$1" "$2" "$3")
    case "$state" in
        *"$4"*) echo "state $1,$2,$3: $state" ;;
        *) fail "expected state at $1,$2,$3 to contain $4, got '$state'" ;;
    esac
}
expect_state_is() { # x y z expected exact state text
    local state
    state=$(state_at "$1" "$2" "$3")
    [ "$state" = "$4" ] || fail "expected $1,$2,$3 to be $4, got '$state'"
    echo "state $1,$2,$3: $state"
}
expect_lamp_at() { # x y z lit
    local state
    state=$(state_at "$1" "$2" "$3")
    case "$state" in
        *"lit=$4"*) echo "lamp $1,$2,$3: $state" ;;
        *) fail "expected lamp at $1,$2,$3 lit=$4, got '$state'" ;;
    esac
}
expect_track_power() { # x y z power
    local state
    state=$(state_at "$1" "$2" "$3")
    case "$state" in
        *"power=$4"*) echo "track $1,$2,$3: $state" ;;
        *) fail "expected track at $1,$2,$3 power=$4, got '$state'" ;;
    esac
}

cleanup() {
    local status=$?
    ctl quit >/dev/null 2>&1 || true
    sleep 1
    rm -rf "$SCRATCH" 2>/dev/null || true
    # Last line of every run: `check-e2e-logs.py` reads it as the verdict.
    echo "EXIT:$status"
}
trap cleanup EXIT

# In EMBEDDED=1 there is no `--dev-mod`: the bundle's own content file is the
# only source of the blocks and the pen.
launch() { # launch [WORLD]
    local world=${1:-$WORLD}
    rm -f "$SOCK"
    cd "$SCRATCH"
    local args=(--game "$GAME" --world "$world" --seed 20261104 --creative --headless --wait
                --resources "$RES")
    if [ "$EMBEDDED" = 1 ]; then
        echo "  launch: no --dev-mod; the bundle carries its own content" >&2
    else
        args+=(--dev-mod "$PORT/content")
    fi
    "$CTL" --socket "$SOCK" launch "${args[@]}"
    ctl wait-ready 120
}

# The `.horizon-cache` folder the helper writes into is relative to the game's
# working directory; accept the scratch root or its game folder, so the check
# does not depend on the launcher's cwd choice.
cache_file() { # cache_file <name>
    local name=$1 path
    for path in "$SCRATCH/.horizon-cache/$name" "$SCRATCH/game/.horizon-cache/$name"; do
        if [ -f "$path" ]; then printf '%s' "$path"; return 0; fi
    done
    return 1
}

# The control for EMBEDDED=1: the same binary (every hook is in it) with the
# embedded content made unavailable — `.horizon-cache` is a plain file, so the
# helper can neither compare nor write its content file and falls back to
# `Continue`. The block and the item must then be unknown; if they resolve, the
# run proves nothing about the embedded file.
control_leg() {
    echo "== control: the same game with no embedded content file =="
    rm -rf "$SCRATCH/.horizon-cache" "$SCRATCH/game/.horizon-cache"
    mkdir -p "$SCRATCH/game"
    : > "$SCRATCH/.horizon-cache"
    : > "$SCRATCH/game/.horizon-cache"
    launch "$CONTROL_WORLD"
    ctl command "difficulty peaceful" >/dev/null
    ctl command "execute in minecraft:the_end run tp @s 0.5 120 0.5" >/dev/null
    ctl wait 10 >/dev/null
    local reply name
    reply=$(ctl command "execute in minecraft:the_end run setblock 0 119 0 redstonepen:track[axis=x,power=0]" 2>&1 || true)
    echo "  setblock reply: $(tr -d '\n' <<<"$reply" | cut -c1-200)"
    ctl command "give @s redstonepen:pen" >/dev/null 2>&1 || true
    ctl command "give @s minecraft:stick" >/dev/null
    ctl wait 4 >/dev/null
    name=$(ctl block 0 119 0 | jq -r '.name // .block // empty')
    [ "$name" = "minecraft:air" ] || fail "the control game holds $name, so the track block id was known"
    ctl inventory | jq -e '[.hotbar[], .main[] | select(.item == "redstonepen:pen")] | length == 0' >/dev/null \
        || fail "the control game has the pen item, so the content did not come from the embedded file"
    ctl inventory | jq -e '[.hotbar[], .main[] | select(.item == "minecraft:stick")] | length >= 1' >/dev/null \
        || fail "the control game cannot even give a vanilla item: the check is not a valid control"
    [ -f "$SCRATCH/.horizon-cache" ] && [ -f "$SCRATCH/game/.horizon-cache" ] \
        || fail "the control's .horizon-cache was replaced: the helper wrote its content"
    echo "  without the embedded file: no pen item, the block stayed $name, a vanilla stick still lands"
    ctl quit >/dev/null 2>&1 || true
    # The next launch reuses the scratch home: wait until this game is gone so
    # its exit cannot race the main run's own content write.
    for _ in $(seq 1 60); do
        ctl state >/dev/null 2>&1 || break
        sleep 0.5
    done
    rm -f "$SCRATCH/.horizon-cache" "$SCRATCH/game/.horizon-cache"
}

# EMBEDDED=1: the bundle must have written its content out at startup, byte for
# byte the delivered content mod.
embedded_content_check() {
    local cache
    for _ in $(seq 1 20); do
        cache=$(cache_file redstonepen-embedded-content.hmod) && break
        sleep 0.5
    done
    if [ -z "${cache:-}" ] || [ ! -f "$cache" ]; then
        echo "  no embedded content written; cache locations seen:" >&2
        find "$SCRATCH" -maxdepth 3 -name '.horizon-cache' -o -maxdepth 3 -name '*embedded-content.hmod' 2>/dev/null | head -5 >&2
        fail "the bundle did not write its embedded content into .horizon-cache/"
    fi
    cmp -s "$cache" "$PORT/content/redstonepen.hmod" \
        || fail "the embedded content differs from content/redstonepen.hmod"
    echo "  embedded content: $(wc -c < "$cache" | tr -d ' ') B at $cache, identical to content/redstonepen.hmod"
}

echo "== launch (The End, creative, redstonepen content) =="
if [ "$CONTROL" = 1 ]; then control_leg; fi
launch
if [ "$EMBEDDED" = 1 ]; then embedded_content_check; fi
ctl command "difficulty peaceful" >/dev/null
ctl command "gamerule doMobSpawning false" >/dev/null
ctl command "kill @e[type=!minecraft:player,type=!minecraft:item]" >/dev/null || true
ctl command "execute in minecraft:the_end run tp @s 0.5 64 0.5" >/dev/null
ctl wait 10
fill -2 63 -2 30 63 30 minecraft:stone
fill -2 64 -2 30 68 30 minecraft:air
# A platform under the slice-4 T rigs (the End is void south of the spawn
# island) and clear air above it.
fill 2 63 -30 20 63 -2 minecraft:stone
fill 2 64 -30 20 68 -2 minecraft:air
ctl wait 3

echo "== five tracks carry a source and light the far lamp =="
setblk 6 64 0 minecraft:redstone_lamp
for x in 1 2 3 4 5; do
    setblk "$x" 64 0 'redstonepen:track[axis=x,power=0]'
done
setblk 0 64 0 minecraft:redstone_block
ctl wait 12
expect_lamp_at 6 64 0 true
expect_track_power 1 64 0 15
expect_track_power 2 64 0 14
expect_track_power 3 64 0 13
expect_track_power 4 64 0 12
expect_track_power 5 64 0 11

echo "== cutting the middle darkens the lamp and drains the far side =="
setblk 3 64 0 minecraft:air
ctl wait 10
expect_lamp_at 6 64 0 false
expect_track_power 5 64 0 0
setblk 3 64 0 'redstonepen:track[axis=x,power=0]'
ctl wait 10
expect_lamp_at 6 64 0 true
expect_track_power 5 64 0 11

echo "== twenty tracks decay to zero: the far lamp stays dark =="
setblk 21 64 6 minecraft:redstone_lamp
for x in $(seq 1 20); do
    setblk "$x" 64 6 'redstonepen:track[axis=x,power=0]'
done
setblk 0 64 6 minecraft:redstone_block
ctl wait 16
expect_lamp_at 21 64 6 false
expect_track_power 5 64 6 11
expect_track_power 15 64 6 1
expect_track_power 16 64 6 0
expect_track_power 20 64 6 0

if [ "$SELFTEST" = 1 ]; then
    # Deliberate corruption: the 20-line's far lamp is dark. This proves the
    # assertions can fail; tools/e2e.sh exits 1 here.
    echo "SELFTEST: expecting the dark far lamp to be lit"
    expect_lamp_at 21 64 6 true
fi

echo "== a relay re-strengthens a decayed line =="
setblk 21 64 12 minecraft:redstone_lamp
for x in $(seq 1 10); do
    setblk "$x" 64 12 'redstonepen:track[axis=x,power=0]'
done
setblk 11 64 12 'redstonepen:relay[facing=east,powered=false]'
for x in $(seq 12 20); do
    setblk "$x" 64 12 'redstonepen:track[axis=x,power=0]'
done
setblk 0 64 12 minecraft:redstone_block
ctl wait 16
expect_state_has 11 64 12 'powered=true'
expect_track_power 10 64 12 6
expect_track_power 12 64 12 15
expect_track_power 20 64 12 7
expect_lamp_at 21 64 12 true

echo "== counterfactual: a plain track instead of the relay drops the far line =="
setblk 11 64 12 'redstonepen:track[axis=x,power=0]'
ctl wait 14
expect_track_power 11 64 12 5
expect_track_power 20 64 12 0
expect_lamp_at 21 64 12 false

echo "== a corner turns the line inside one block =="
setblk 12 64 -6 minecraft:redstone_lamp
setblk 12 64 -5 'redstonepen:track[axis=z,bend=none,power=0]'
setblk 12 64 -4 'redstonepen:track[axis=z,bend=north_west,power=0]'
setblk 11 64 -4 'redstonepen:track[axis=x,bend=none,power=0]'
setblk 10 64 -4 minecraft:redstone_block
ctl wait 14
expect_track_power 11 64 -4 15
expect_track_power 12 64 -4 14
expect_track_power 12 64 -5 13
expect_lamp_at 12 64 -6 true
expect_state_has 12 64 -4 'bend=north_west'

if [ "$SELFTEST_CORNER" = 1 ]; then
    # Deliberate corruption: the far corner lamp is lit. This proves the
    # corner assertion can fail; tools/e2e.sh exits 1 here.
    echo "SELFTEST_CORNER: expecting the lit far corner lamp to be dark"
    expect_lamp_at 12 64 -6 false
fi

echo "== control: a straight track at the turn cannot cross the axis =="
setblk 12 64 -4 'redstonepen:track[axis=x,bend=none,power=0]'
ctl wait 12
expect_lamp_at 12 64 -6 false
expect_track_power 12 64 -5 0
expect_track_power 11 64 -4 15

echo "== control: a corner with the wrong arms is dark =="
setblk 12 64 -4 'redstonepen:track[axis=z,bend=north_east,power=0]'
ctl wait 12
expect_lamp_at 12 64 -6 false
expect_track_power 12 64 -5 0

echo "== cutting the corner darkens the far arm, restoring it lights it =="
setblk 12 64 -4 minecraft:air
ctl wait 12
expect_lamp_at 12 64 -6 false
expect_track_power 12 64 -5 0
setblk 12 64 -4 'redstonepen:track[axis=z,bend=north_west,power=0]'
ctl wait 12
expect_lamp_at 12 64 -6 true
expect_track_power 12 64 -4 14
expect_track_power 12 64 -5 13

echo "== stone control: a stone block at the turn is dark =="
setblk 12 64 -4 minecraft:stone
ctl wait 12
expect_lamp_at 12 64 -6 false
setblk 12 64 -4 minecraft:air

echo "== straight control: a two-track line carries the same decay =="
setblk 13 64 4 minecraft:redstone_lamp
setblk 12 64 4 'redstonepen:track[axis=x,bend=none,power=0]'
setblk 11 64 4 'redstonepen:track[axis=x,bend=none,power=0]'
setblk 10 64 4 minecraft:redstone_block
ctl wait 12
expect_track_power 11 64 4 15
expect_track_power 12 64 4 14
expect_lamp_at 13 64 4 true

echo "== a T junction carries one power to two arms =="
# Arms west, east and south in one cell: the source feeds the west arm and
# both other arms derive from the T's single net power.
setblk 8 64 -10 minecraft:redstone_lamp
setblk 6 64 -8 minecraft:redstone_lamp
setblk 7 64 -10 'redstonepen:track[axis=x,bend=none,power=0]'
setblk 6 64 -10 'redstonepen:track[axis=z,bend=south_west_east,power=0]'
setblk 5 64 -10 'redstonepen:track[axis=x,bend=none,power=0]'
setblk 4 64 -10 minecraft:redstone_block
setblk 6 64 -9 'redstonepen:track[axis=z,bend=none,power=0]'
ctl wait 14
expect_state_has 6 64 -10 'bend=south_west_east'
expect_track_power 5 64 -10 15
expect_track_power 6 64 -10 14
expect_track_power 7 64 -10 13
expect_track_power 6 64 -9 13
expect_lamp_at 8 64 -10 true
expect_lamp_at 6 64 -8 true

if [ "$SELFTEST_T" = 1 ]; then
    # Deliberate corruption: both T lamps are lit. This proves the T assertion
    # can fail; tools/e2e.sh exits 1 here.
    echo "SELFTEST_T: expecting the lit second T lamp to be dark"
    expect_lamp_at 6 64 -8 false
fi

echo "== cutting one T arm leaves the other side lit =="
setblk 7 64 -10 minecraft:air
ctl wait 12
expect_lamp_at 8 64 -10 false
expect_lamp_at 6 64 -8 true
expect_track_power 6 64 -9 13
setblk 7 64 -10 'redstonepen:track[axis=x,bend=none,power=0]'
ctl wait 12
expect_lamp_at 8 64 -10 true
expect_lamp_at 6 64 -8 true
echo "== cutting the other T arm leaves the first side lit =="
setblk 6 64 -9 minecraft:air
ctl wait 12
expect_lamp_at 6 64 -8 false
expect_lamp_at 8 64 -10 true
setblk 6 64 -9 'redstonepen:track[axis=z,bend=none,power=0]'
ctl wait 12
expect_lamp_at 6 64 -8 true

echo "== control: a T missing the fed arm leaves both lamps dark =="
setblk 6 64 -10 'redstonepen:track[axis=z,bend=north_south_east,power=0]'
ctl wait 12
expect_track_power 6 64 -10 0
expect_lamp_at 8 64 -10 false
expect_lamp_at 6 64 -8 false

echo "== control: a T with the wrong third arm leaves that side dark =="
setblk 6 64 -10 'redstonepen:track[axis=z,bend=north_west_east,power=0]'
ctl wait 12
expect_track_power 6 64 -10 14
expect_lamp_at 8 64 -10 true
expect_lamp_at 6 64 -8 false
expect_track_power 6 64 -9 0

echo "== control: a straight track at the junction leaves the branch dark =="
setblk 6 64 -10 'redstonepen:track[axis=x,bend=none,power=0]'
ctl wait 12
expect_lamp_at 8 64 -10 true
expect_lamp_at 6 64 -8 false
expect_track_power 6 64 -9 0
setblk 6 64 -10 'redstonepen:track[axis=z,bend=south_west_east,power=0]'
ctl wait 12
expect_lamp_at 8 64 -10 true
expect_lamp_at 6 64 -8 true

echo "== stone control: a stone block emits nothing =="
setblk 2 64 18 minecraft:redstone_lamp
setblk 1 64 18 minecraft:stone
setblk 0 64 18 minecraft:redstone_block
ctl wait 8
expect_lamp_at 2 64 18 false
echo "== an unpowered track line emits nothing =="
setblk 6 64 18 minecraft:redstone_lamp
for x in 3 4 5; do
    setblk "$x" 64 18 'redstonepen:track[axis=x,power=0]'
done
ctl wait 8
expect_lamp_at 6 64 18 false
expect_track_power 5 64 18 0

echo "== inverted relay: powered is dark, unpowered is lit =="
setblk 2 64 20 minecraft:redstone_lamp
setblk 1 64 20 'redstonepen:inverted_relay[facing=east,powered=false]'
setblk 0 64 20 minecraft:redstone_block
ctl wait 10
expect_state_has 1 64 20 'powered=true'
expect_lamp_at 2 64 20 false
if [ "$SELFTEST_RELAY" = 1 ]; then
    # Deliberate corruption: the powered inverted relay is dark. This proves
    # the variant assertion can fail; tools/e2e.sh exits 1 here.
    echo "SELFTEST_RELAY: expecting the powered inverted lamp to be lit"
    expect_lamp_at 2 64 20 true
fi
setblk 0 64 20 minecraft:air
ctl wait 10
expect_state_has 1 64 20 'powered=false'
expect_lamp_at 2 64 20 true

echo "== control: the same rig with a plain relay is dark when its input falls =="
setblk 2 64 22 minecraft:redstone_lamp
setblk 1 64 22 'redstonepen:relay[facing=east,powered=false]'
setblk 0 64 22 minecraft:redstone_block
ctl wait 10
expect_state_has 1 64 22 'powered=true'
expect_lamp_at 2 64 22 true
setblk 0 64 22 minecraft:air
ctl wait 10
expect_state_has 1 64 22 'powered=false'
expect_lamp_at 2 64 22 false

echo "== pulse relay: a rising edge emits a pulse that clears; bistable latches it =="
setblk 3 64 24 minecraft:redstone_lamp
setblk 2 64 24 'redstonepen:bistable_relay[facing=east,powered=false,state=0]'
setblk 1 64 24 'redstonepen:pulse_relay[facing=east,powered=false,state=0]'
setblk 0 64 24 minecraft:redstone_block
ctl wait 10
# The pulse has cleared (state=0) while its input is still powered, and the
# bistable captured the pulse's rising edge (state=1, lamp lit).
expect_state_has 1 64 24 'powered=true'
expect_state_has 1 64 24 'state=0'
expect_state_has 2 64 24 'state=1'
expect_lamp_at 3 64 24 true
# Removing the input holds the bistable's latched output.
setblk 0 64 24 minecraft:air
ctl wait 10
expect_state_has 1 64 24 'powered=false'
expect_state_has 2 64 24 'state=1'
expect_lamp_at 3 64 24 true
# A second rising edge pulses again and toggles the bistable off.
setblk 0 64 24 minecraft:redstone_block
ctl wait 10
expect_state_has 1 64 24 'powered=true'
expect_state_has 1 64 24 'state=0'
expect_state_has 2 64 24 'state=0'
expect_lamp_at 3 64 24 false

echo "== bistable relay: toggles on an edge and holds when the input falls =="
setblk 2 64 16 minecraft:redstone_lamp
setblk 1 64 16 'redstonepen:bistable_relay[facing=east,powered=false,state=0]'
setblk 0 64 16 minecraft:redstone_block
ctl wait 10
expect_state_has 1 64 16 'powered=true'
expect_state_has 1 64 16 'state=1'
expect_lamp_at 2 64 16 true
setblk 0 64 16 minecraft:air
ctl wait 10
expect_state_has 1 64 16 'powered=false'
expect_state_has 1 64 16 'state=1'
expect_lamp_at 2 64 16 true
setblk 0 64 16 minecraft:redstone_block
ctl wait 10
expect_state_has 1 64 16 'powered=true'
expect_state_has 1 64 16 'state=0'
expect_lamp_at 2 64 16 false

echo "== pen: a click on a support draws a track on the clicked face =="
setblk 0 64 26 minecraft:stone
ctl command "execute in minecraft:the_end run tp @s 0.5 64 28.5" >/dev/null
ctl wait 6
ctl look-at-block 0 64 26 up >/dev/null
ctl wait 2
ctl give "$TOOL" 1 >/dev/null || fail "cannot give $TOOL"
ctl wait 2
ctl select "$TOOL" >/dev/null || fail "cannot select $TOOL"
held=$(ctl inventory | jq -r '.held.item // "none"')
echo "held: $held"
[ "$held" = "$TOOL" ] || fail "the held item is $held, expected $TOOL"
ctl interact 0 64 26 up
ctl wait 6
if [ "$COUNTERFACTUAL" = 1 ]; then
    expect_state_is 0 65 26 minecraft:air
    echo "counterfactual: a plain stick drew no track"
else
    expect_state_has 0 65 26 'axis=z'
    expect_state_has 0 65 26 'bend=none'
    expect_state_has 0 65 26 'power=0'

    echo "== pen: a click on the track's end extends it =="
    ctl interact 0 65 26 south
    ctl wait 6
    expect_state_has 0 65 27 'axis=z'
    expect_state_has 0 65 27 'bend=none'

    echo "== pen: a click across an unwired track erases it =="
    # A wired track's side click bends it (slice 3), so cut the extension
    # before erasing the original.
    setblk 0 65 27 minecraft:air
    setblk 0 65 28 minecraft:stone
    ctl command "execute in minecraft:the_end run tp @s 0.5 66 28.5" >/dev/null
    ctl wait 4
    ctl interact 0 65 26 up
    ctl wait 6
    expect_state_is 0 65 26 minecraft:air

    echo "== pen: a side click bends a wired track, its new arm extends =="
    setblk 24 64 28 minecraft:stone
    ctl command "execute in minecraft:the_end run tp @s 24.5 66 30.5" >/dev/null
    ctl wait 4
    ctl look 90 45
    ctl wait 2
    ctl interact 24 64 28 up
    ctl wait 6
    expect_state_has 24 65 28 'axis=x'
    expect_state_has 24 65 28 'bend=none'

    # A click on the east end draws the next straight track; stand east of it.
    ctl command "execute in minecraft:the_end run tp @s 26.5 66 28.5" >/dev/null
    ctl wait 3
    ctl look 90 20
    ctl wait 2
    ctl interact 24 65 28 east
    ctl wait 6
    expect_state_has 25 65 28 'axis=x'
    expect_state_has 25 65 28 'bend=none'

    # The west arm is wired to the first track, so the north side bends it.
    ctl command "execute in minecraft:the_end run tp @s 25.5 66 26.5" >/dev/null
    ctl wait 3
    ctl look 0 20
    ctl wait 2
    ctl interact 25 65 28 north
    ctl wait 6
    expect_state_has 25 65 28 'bend=north_west'

    # North is an arm now, so the same face extends the corner's new line.
    ctl interact 25 65 28 north
    ctl wait 6
    expect_state_has 25 65 27 'axis=z'
    expect_state_has 25 65 27 'bend=none'

    setblk 25 65 26 minecraft:redstone_lamp
    setblk 23 65 28 minecraft:redstone_block
    ctl wait 14
    expect_track_power 24 65 28 15
    expect_track_power 25 65 28 14
    expect_track_power 25 65 27 13
    expect_lamp_at 25 65 26 true

    echo "== pen-drawn L: cutting the corner darkens the far lamp =="
    setblk 25 65 28 minecraft:air
    ctl wait 12
    expect_lamp_at 25 65 26 false

    echo "== pen: a side click on a two-wired-arm track widens it into a T =="
    # The centre track's west and east arms both reach a track, and the north
    # cell is air, so a north side click adds the third arm.
    setblk 10 64 -16 minecraft:redstone_block
    setblk 14 64 -16 minecraft:redstone_lamp
    setblk 11 64 -16 'redstonepen:track[axis=x,bend=none,power=0]'
    setblk 12 64 -16 'redstonepen:track[axis=x,bend=none,power=0]'
    setblk 13 64 -16 'redstonepen:track[axis=x,bend=none,power=0]'
    ctl wait 12
    expect_track_power 12 64 -16 14
    expect_lamp_at 14 64 -16 true
    ctl command "execute in minecraft:the_end run tp @s 12.5 64 -18.5" >/dev/null
    ctl wait 4
    ctl look-at-block 12 64 -16 north >/dev/null
    ctl wait 2
    ctl interact 12 64 -16 north
    ctl wait 8
    expect_state_has 12 64 -16 'bend=north_west_east'
    expect_track_power 12 64 -16 14
    # The new north arm reaches a branch track and a second lamp.
    setblk 12 64 -18 minecraft:redstone_lamp
    setblk 12 64 -17 'redstonepen:track[axis=z,bend=none,power=0]'
    ctl wait 12
    expect_track_power 12 64 -16 14
    expect_track_power 12 64 -17 13
    expect_lamp_at 14 64 -16 true
    expect_lamp_at 12 64 -18 true

    echo "== pen: sneak-click drops a T's wired arm and keeps its neighbours =="
    # The south arm reaches a vanilla wire: non-solid, so the face stays
    # clickable from the south, and any adjacent wire counts as a wired arm.
    # The west and east arms carry the source and the lamp.
    setblk 8 64 -22 minecraft:redstone_lamp
    setblk 7 64 -22 'redstonepen:track[axis=x,bend=none,power=0]'
    setblk 6 64 -22 'redstonepen:track[axis=z,bend=south_west_east,power=0]'
    setblk 5 64 -22 'redstonepen:track[axis=x,bend=none,power=0]'
    setblk 4 64 -22 minecraft:redstone_block
    setblk 6 64 -21 minecraft:redstone_wire
    ctl wait 12
    expect_track_power 6 64 -22 14
    expect_lamp_at 8 64 -22 true
    ctl command "execute in minecraft:the_end run tp @s 6.5 64 -19.5" >/dev/null
    ctl wait 4
    ctl look-at-block 6 64 -22 south >/dev/null
    ctl wait 2
    ctl key shift down
    ctl wait 2
    ctl interact 6 64 -22 south
    ctl wait 8
    expect_state_has 6 64 -22 'axis=x'
    expect_state_has 6 64 -22 'bend=none'
    expect_track_power 6 64 -22 14
    expect_lamp_at 8 64 -22 true
    ctl key shift up
    ctl wait 2

    echo "== pen: sneak-click erases a wired track without cutting neighbours =="
    ctl command "execute in minecraft:the_end run tp @s 12.5 64 -13.5" >/dev/null
    ctl wait 4
    ctl look-at-block 12 64 -16 south >/dev/null
    ctl wait 2
    ctl key shift down
    ctl wait 2
    ctl interact 12 64 -16 south
    ctl wait 8
    expect_state_is 12 64 -16 minecraft:air
    expect_lamp_at 14 64 -16 false
    expect_lamp_at 12 64 -18 false
    expect_track_power 12 64 -17 0

    if [ "$SELFTEST_ERASE" = 1 ]; then
        # Deliberate corruption: the erased track's lamp cannot be lit. This
        # proves the sneak-erase assertion can fail; tools/e2e.sh exits 1 here.
        echo "SELFTEST_ERASE: expecting the erased wired lamp to be lit"
        expect_lamp_at 14 64 -16 true
    fi

    echo "== pen: a sneak click on a support draws nothing =="
    ctl command "execute in minecraft:the_end run tp @s 16.5 64 -18.5" >/dev/null
    ctl wait 4
    # `ctl interact` releases the agent's held keys when its task ends, so the
    # sneak modifier has to be pressed again before this click.
    ctl key shift down
    ctl wait 2
    ctl interact 16 63 -16 up
    ctl wait 6
    expect_state_is 16 64 -16 minecraft:air
    ctl key shift up
    ctl wait 2
fi

# Stand near the acceptance grids so their chunks stay loaded while the states
# are placed; the tee grid reaches z=115.
ctl command "execute in minecraft:the_end run tp @s 16.5 64 20" >/dev/null
ctl wait 10
setblk 35 70 76 minecraft:stone
ctl command "execute in minecraft:the_end run tp @s 35.5 71 76.5" >/dev/null
ctl wait 10

echo "== all 48 straight track states are accepted =="
track_count=0
for axis in x y z; do
    for power in $(seq 0 15); do
        setblk "$track_count" 70 40 "redstonepen:track[axis=$axis,bend=none,power=$power]"
        track_count=$((track_count + 1))
    done
done
echo "accepted $track_count track states"
[ "$track_count" = 48 ] || fail "expected 48 track states, set $track_count"
ctl wait 3
expect_state_has 0 70 40 'axis=x'
expect_state_has 0 70 40 'bend=none'
expect_state_has 0 70 40 'power=0'
expect_state_has 16 70 40 'axis=y'
expect_state_has 32 70 40 'axis=z'

echo "== all 192 corner track states are accepted =="
corner_count=0
for bend in down_north down_south down_west down_east \
    up_north up_south up_west up_east \
    north_west north_east south_west south_east; do
    case "$bend" in
        down_*|up_*) axis=y ;;
        *) axis=z ;;
    esac
    for power in $(seq 0 15); do
        # Three blocks apart: no corner can feed another in this grid.
        setblk "$(( (corner_count % 24) * 3 ))" 72 "$(( 44 + (corner_count / 24) * 3 ))" \
            "redstonepen:track[axis=$axis,bend=$bend,power=$power]"
        corner_count=$((corner_count + 1))
    done
done
echo "accepted $corner_count corner states"
[ "$corner_count" = 192 ] || fail "expected 192 corner states, set $corner_count"
ctl wait 3
expect_state_has 0 72 44 'bend=down_north'
expect_state_has 24 72 59 'bend=north_west'
expect_state_has 24 72 59 'power=0'
expect_state_has 69 72 65 'bend=south_east'

echo "== all 320 T track states are accepted =="
tee_count=0
for bend in down_up_north down_up_south down_up_west down_up_east \
    down_north_south down_north_west down_north_east \
    down_south_west down_south_east down_west_east \
    up_north_south up_north_west up_north_east \
    up_south_west up_south_east up_west_east \
    north_south_west north_south_east north_west_east south_west_east; do
    case "$bend" in
        down_*|up_*) axis=y ;;
        *) axis=z ;;
    esac
    for power in $(seq 0 15); do
        # Three blocks apart: no T can feed another in this grid.
        setblk "$(( (tee_count % 24) * 3 ))" 74 "$(( 76 + (tee_count / 24) * 3 ))" \
            "redstonepen:track[axis=$axis,bend=$bend,power=$power]"
        tee_count=$((tee_count + 1))
    done
done
echo "accepted $tee_count T states"
[ "$tee_count" = 320 ] || fail "expected 320 T states, set $tee_count"
ctl wait 3
expect_state_has 0 74 76 'bend=down_up_north'
expect_state_has 0 74 76 'power=0'
expect_state_has 0 74 112 'bend=north_west_east'
expect_state_has 48 74 112 'bend=south_west_east'

echo "== all 72 relay states are accepted =="
relay_count=0
for spec in \
    'redstonepen:relay' \
    'redstonepen:inverted_relay' \
    'redstonepen:pulse_relay' \
    'redstonepen:bistable_relay'; do
    case "$spec" in
        *pulse_relay|*bistable_relay) states='0 1' ;;
        *) states='-' ;;
    esac
    for facing in down up north south west east; do
        for powered in false true; do
            for state in $states; do
                props="facing=$facing,powered=$powered"
                [ "$state" = '-' ] || props="$props,state=$state"
                # Three blocks apart: no relay output can reach a neighbour in
                # this acceptance grid, so the states stay as placed.
                setblk "$((relay_count * 3))" 70 42 "$spec[$props]"
                relay_count=$((relay_count + 1))
            done
        done
    done
done
echo "accepted $relay_count relay states"
[ "$relay_count" = 72 ] || fail "expected 72 relay states, set $relay_count"
ctl wait 3
expect_state_has 0 70 42 'facing=down'
expect_state_has 0 70 42 'powered=false'
expect_state_has 36 70 42 'facing=down'
expect_state_has 72 70 42 'facing=down'
expect_state_has 72 70 42 'state=0'
expect_state_has 144 70 42 'facing=down'
expect_state_has 144 70 42 'state=0'

echo "== undeclared properties and bad values are rejected =="
if ctl setblock 20 70 40 'redstonepen:track[axis=w,power=0]' >/dev/null 2>&1; then
    fail "track[axis=w] was accepted"
fi
echo "rejected as expected: redstonepen:track[axis=w,power=0]"
if ctl setblock 20 70 40 'redstonepen:track[axis=x,bend=sideways,power=0]' >/dev/null 2>&1; then
    fail "track[bend=sideways] was accepted"
fi
echo "rejected as expected: redstonepen:track[axis=x,bend=sideways,power=0]"
if ctl setblock 20 70 40 'redstonepen:track[axis=x,bend=north_west_east_up,power=0]' >/dev/null 2>&1; then
    fail "track[bend=north_west_east_up] was accepted"
fi
echo "rejected as expected: redstonepen:track[axis=x,bend=north_west_east_up,power=0]"
if ctl setblock 20 70 40 'redstonepen:track[axis=x,bend=north_south,power=0]' >/dev/null 2>&1; then
    fail "track[bend=north_south] was accepted"
fi
echo "rejected as expected: redstonepen:track[axis=x,bend=north_south,power=0]"
if ctl setblock 20 70 42 'redstonepen:relay[channel=3]' >/dev/null 2>&1; then
    fail "relay[channel=3] was accepted"
fi
echo "rejected as expected: redstonepen:relay[channel=3]"
if ctl setblock 21 70 42 'redstonepen:pulse_relay[state=2]' >/dev/null 2>&1; then
    fail "pulse_relay[state=2] was accepted"
fi
echo "rejected as expected: redstonepen:pulse_relay[state=2]"
if ctl setblock 22 70 42 'redstonepen:inverted_relay[state=1]' >/dev/null 2>&1; then
    fail "inverted_relay[state=1] was accepted"
fi
echo "rejected as expected: redstonepen:inverted_relay[state=1]"

echo "== items and creative content resolve =="
whence=$(ctl give redstonepen:pen 1 2>&1 || true)
case "$whence" in
    *'"given":1'* | *"Gave 1"*) echo "gave redstonepen:pen 1" ;;
    *) fail "give redstonepen:pen answered: $whence" ;;
esac
for item in relay inverted_relay pulse_relay bistable_relay; do
    whence=$(ctl give "redstonepen:$item" 1 2>&1 || true)
    case "$whence" in
        *'"given":1'* | *"Gave 1"*) echo "gave redstonepen:$item 1" ;;
        *) fail "give redstonepen:$item answered: $whence" ;;
    esac
done

# Keep the client's built state table and content warnings for the offline
# binding check (the scratch home is deleted below).
STATES=$(find "$SCRATCH/.horizon-cache/mod-content" -name model-states.json 2>/dev/null | head -1)
if [ -n "$STATES" ]; then
    cp "$STATES" /tmp/opencode/redstonepen-model-states.json
fi
WARNINGS=$(find "$SCRATCH/.horizon-cache/mod-content" -name horizon-mod-content-warnings.txt 2>/dev/null | head -1)
if [ -n "$WARNINGS" ]; then
    cp "$WARNINGS" /tmp/opencode/redstonepen-content-warnings.txt
fi

if [ "$SELFTEST" = 1 ] || [ "$SELFTEST_CORNER" = 1 ] || [ "$SELFTEST_T" = 1 ] || [ "$SELFTEST_ERASE" = 1 ]; then
    echo "PASS (selftest?)" # unreachable: the corrupted assertion above exits
elif [ "$COUNTERFACTUAL" = 1 ]; then
    echo "PASS (counterfactual, plain minecraft:stick): the support click drew no track; the signal rigs still pass"
else
    echo "PASS: 5-track line lights its lamp (15..11), cutting darkens it, 20 tracks decay to zero at 16, a relay re-strengthens 12..20 and the far lamp is lit, replacing it drops to 0, a corner turns the line (15,14,13) and a straight track at the turn, a wrong bend, a cut corner and stone are all dark, a two-track straight line is 15,14, a T junction feeds both derived arms (15,14,13,13), cutting either arm keeps the other lit and the wrong/fourth-side/straight controls are dark, the pen draws/extends/erases/bends/widens into a T and the drawn L carries 15,14,13, a sneak click drops a T's wired arm and erases a wired track cell while a plain click still bends, and a sneak click on a support draws nothing, stone and an unsourced line emit nothing, an inverted relay is dark when powered and lit when not, a pulse relay clears while a plain relay holds and a bistable latches and toggles on edges, 48 straight + 192 corner + 320 T track + 72 relay states accepted"
fi
