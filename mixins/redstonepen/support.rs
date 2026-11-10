// Game-side glue for the Redstone Pen track slice: the pen click (draw,
// extend, bend, widen, erase), the track power solve, the relay, and the
// signal answers.
//
// Injected into `crate::game` as `redstonepen_state`; it needs the game, so it
// is not part of the standalone library (only `runtime.rs` is).
//
// Upstream keeps 24 wire segments in a block entity and recomputes each net
// from every external neighbour. This slice has one track per block, whose
// two or three arms (`axis` + `bend`) and power are block-state properties, so
// the solve is a small worklist over the tracks a neighbour change queued. It
// runs at `level_tick_post`, where the level is in place and `native_set_block`
// works (the pin has no mutable neighbour hook), and it is bounded by step and
// visit budgets.
//
// A track answers its power on its arms only. A track-to-track step costs one,
// a vanilla redstone wire costs one, and any other source seeds its full
// strength (upstream's `handleNetNeighborChanged`); a relay re-emits 15 from
// its output face. The relay's `facing` is its output direction in this slice
// (upstream's is the mounting face plus a rotation).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Mutex;

use horizon_core::geometry::Direction as GeoDir;
use horizon_core::position::BlockPos;
use horizon_world::level::Dimension as LevelDimension;
use horizon_world::level::{PlacementContext, flags};
use horizon_world::registry::{BlockRegistry, BlockStateId};

use crate::game::redstonepen::{
    Axis, ClickAction, ClickMode, Dir, PULSE_TICKS, RELAY_OFF_DELAY_TICKS, RelayKind, RelayPhase,
    Shape, Supply, click_action, connected, placement_axis, relay_delayed_off, relay_edge,
    relay_facing, relay_notifies, relay_powered, relay_variant_signal, track_power, track_signal,
};

/// Entries one tick may apply; a safety budget against runaway feedback.
const MAX_DIRTY: usize = 4096;
/// Track recomputes one level tick may do (all its tracks together).
const MAX_STEPS: usize = 16_384;
/// Times one track may be recomputed in a single level tick.
const MAX_VISITS: u8 = 8;
/// The one track block this slice ships (`RedstoneTrack.RedstoneTrackBlock`).
const TRACK_BLOCK: &str = "redstonepen:track";
/// The pen (`ModContent.initItems`); upstream's `quill` is the same item logic.
const PEN_ITEM: &str = "redstonepen:pen";
/// The vanilla wire the pen can also draw into (`RedStoneWireBlock.POWER`).
const WIRE_BLOCK: &str = "minecraft:redstone_wire";

#[derive(Default)]
struct State {
    /// Tracks queued for the next power solve.
    tracks: BTreeSet<(String, [i32; 3])>,
    /// Relays queued for the next input evaluation.
    relays: BTreeSet<(String, [i32; 3])>,
    /// Delayed relay writes (`scheduleTick(pos, this, 2)` upstream).
    pending: BTreeMap<(String, [i32; 3]), Pending>,
    /// Positions this port is writing through `native_set_block` right now.
    /// The engine reports the write as `block_changed`; that is a state
    /// refinement, not a placement, so it must not seed another evaluation.
    writing: BTreeSet<(String, [i32; 3])>,
}

/// A write upstream would deliver with a 2-tick block tick.
#[derive(Clone, Copy)]
struct Pending {
    ticks: u8,
    kind: RelayKind,
    write: PendingWrite,
    notify: bool,
}

#[derive(Clone, Copy)]
enum PendingWrite {
    /// Turn a plain/inverted relay's output off after the delay.
    Phase(RelayPhase),
    /// Clear a pulse relay's `STATE` after its 2-tick pulse.
    PulseClear,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn state() -> std::sync::MutexGuard<'static, Option<State>> {
    STATE.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// A track read from its state: its two or three arms and one power.
struct Track {
    shape: Shape,
    power: u8,
}

/// A relay read from its state (`facing` is this slice's output direction).
struct Relay {
    kind: RelayKind,
    output: Dir,
    phase: RelayPhase,
}

fn track(registry: &BlockRegistry, state: BlockStateId) -> Option<Track> {
    let state = registry.state(state)?;
    if &*state.name != TRACK_BLOCK {
        return None;
    }
    let axis = Axis::from_name(state.properties.get("axis")?)?;
    // A state always carries `bend`; a state written before slice 3 (or by
    // hand) without it is a straight track on its axis.
    let bend = state
        .properties
        .get("bend")
        .map_or("none", String::as_str);
    let shape = Shape::from_bend(axis, bend)?;
    let power = state
        .properties
        .get("power")?
        .parse::<u8>()
        .ok()?
        .min(15);
    Some(Track { shape, power })
}

fn relay(registry: &BlockRegistry, state: BlockStateId) -> Option<Relay> {
    let state = registry.state(state)?;
    let kind = RelayKind::from_block_id(&state.name)?;
    let output = dir_of(state.properties.get("facing")?)?;
    let powered = state
        .properties
        .get("powered")
        .is_some_and(|value| value == "true");
    let latched = kind.has_state()
        && state
            .properties
            .get("state")
            .is_some_and(|value| value == "1");
    Some(Relay {
        kind,
        output,
        phase: RelayPhase {
            powered,
            state: latched,
        },
    })
}

fn dir_of(value: &str) -> Option<Dir> {
    Dir::from_name(value)
}

fn to_dir(dir: GeoDir) -> Dir {
    match dir {
        GeoDir::Down => Dir::Down,
        GeoDir::Up => Dir::Up,
        GeoDir::North => Dir::North,
        GeoDir::South => Dir::South,
        GeoDir::West => Dir::West,
        GeoDir::East => Dir::East,
    }
}

fn dir_from_vector([x, y, z]: [i32; 3]) -> Option<Dir> {
    Some(match (x, y, z) {
        (0, -1, 0) => Dir::Down,
        (0, 1, 0) => Dir::Up,
        (0, 0, -1) => Dir::North,
        (0, 0, 1) => Dir::South,
        (-1, 0, 0) => Dir::West,
        (1, 0, 0) => Dir::East,
        _ => return None,
    })
}

fn dimension_name(dimension: LevelDimension) -> &'static str {
    match dimension {
        LevelDimension::Overworld => "minecraft:overworld",
        LevelDimension::Nether => "minecraft:the_nether",
        LevelDimension::End => "minecraft:the_end",
    }
}

fn block_at(game: &crate::game::Game<'_>, [x, y, z]: [i32; 3]) -> Option<BlockStateId> {
    let world = &game.world;
    if !(-30_000_000..30_000_000).contains(&x)
        || !(-30_000_000..30_000_000).contains(&z)
        || !(world.shape.min_y..world.shape.max_y()).contains(&y)
        || !world.loaded(x, z)
    {
        return None;
    }
    Some(world.block(x, y, z))
}

fn level_block(level: &crate::game::GameLevel<'_>, [x, y, z]: [i32; 3]) -> Option<BlockStateId> {
    let world = &*level.world;
    if !(-30_000_000..30_000_000).contains(&x)
        || !(-30_000_000..30_000_000).contains(&z)
        || !(world.shape.min_y..world.shape.max_y()).contains(&y)
        || !world.loaded(x, z)
    {
        return None;
    }
    Some(world.block(x, y, z))
}

fn is_air(game: &crate::game::Game<'_>, pos: [i32; 3]) -> bool {
    block_at(game, pos)
        .and_then(|state| game.pack.assets.registry.state(state))
        .is_some_and(|state| {
            matches!(
                &*state.name,
                "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
            )
        })
}

/// A fresh track state with `shape` (its two or three arms) and `power`.
fn track_state(registry: &BlockRegistry, shape: Shape, power: u8) -> Option<BlockStateId> {
    let mut descriptor = registry.state(registry.default_state(TRACK_BLOCK)?)?.clone();
    descriptor
        .properties
        .insert("axis".to_owned(), shape.axis().name().to_owned());
    descriptor
        .properties
        .insert("bend".to_owned(), shape.bend_name().to_owned());
    descriptor
        .properties
        .insert("power".to_owned(), power.min(15).to_string());
    registry.resolve(&descriptor).ok()
}

/// A relay state with its output face and phase replaced.
fn relay_state(
    registry: &BlockRegistry,
    state: BlockStateId,
    output: Dir,
    phase: RelayPhase,
) -> Option<BlockStateId> {
    let mut descriptor = registry.state(state)?.clone();
    descriptor
        .properties
        .insert("facing".to_owned(), output.name().to_owned());
    descriptor
        .properties
        .insert("powered".to_owned(), phase.powered.to_string());
    if descriptor.properties.contains_key("state") {
        descriptor
            .properties
            .insert("state".to_owned(), u8::from(phase.state).to_string());
    }
    registry.resolve(&descriptor).ok()
}

/// A track state with its `power` replaced.
fn with_power(registry: &BlockRegistry, state: BlockStateId, power: u8) -> Option<BlockStateId> {
    let mut descriptor = registry.state(state)?.clone();
    descriptor
        .properties
        .insert("power".to_owned(), power.min(15).to_string());
    registry.resolve(&descriptor).ok()
}

/// A track state with its `shape` (both `axis` and `bend`) replaced.
fn with_shape(registry: &BlockRegistry, state: BlockStateId, shape: Shape) -> Option<BlockStateId> {
    let mut descriptor = registry.state(state)?.clone();
    descriptor
        .properties
        .insert("axis".to_owned(), shape.axis().name().to_owned());
    descriptor
        .properties
        .insert("bend".to_owned(), shape.bend_name().to_owned());
    registry.resolve(&descriptor).ok()
}

fn air_state(registry: &BlockRegistry) -> Option<BlockStateId> {
    registry.default_state("minecraft:air")
}

fn mark_track(dimension: &str, pos: [i32; 3]) {
    let mut guard = state();
    let state = guard.get_or_insert_with(State::default);
    if state.tracks.len() < MAX_DIRTY {
        state.tracks.insert((dimension.to_owned(), pos));
    }
}

fn mark_relay(dimension: &str, pos: [i32; 3]) {
    let mut guard = state();
    let state = guard.get_or_insert_with(State::default);
    if state.relays.len() < MAX_DIRTY {
        state.relays.insert((dimension.to_owned(), pos));
    }
}

/// Queue every track and relay beside `pos`; a removed source, a newly placed
/// block or a solved power can all change their answer.
fn mark_around(game: &crate::game::Game<'_>, dimension: &str, pos: [i32; 3]) {
    let registry = &game.pack.assets.registry;
    for dir in Dir::ALL {
        let neighbour_pos = dir.offset(pos);
        let Some(neighbour) = block_at(game, neighbour_pos) else {
            continue;
        };
        if track(registry, neighbour).is_some() {
            mark_track(dimension, neighbour_pos);
        } else if relay(registry, neighbour).is_some() {
            mark_relay(dimension, neighbour_pos);
        }
    }
}

/// The signal a block answers toward a reader (the pin has no per-direction
/// read for mods, so this rebuilds it: our own blocks from their state, a
/// vanilla wire from its power, everything else from the behaviour facts).
fn signal_toward(
    game: &crate::game::Game<'_>,
    state: BlockStateId,
    from_reader: Dir,
) -> u8 {
    let registry = &game.pack.assets.registry;
    let Some(block) = registry.state(state) else {
        return 0;
    };
    if let Some(track) = track(registry, state) {
        return u8::try_from(track_signal(track.power, track.shape, from_reader.opposite()))
            .unwrap_or(0);
    }
    if let Some(relay) = relay(registry, state) {
        return u8::try_from(relay_variant_signal(
            relay.kind,
            relay.phase,
            relay.output,
            from_reader.opposite(),
        ))
        .unwrap_or(0);
    }
    if &*block.name == WIRE_BLOCK {
        return block
            .properties
            .get("power")
            .and_then(|power| power.parse::<u8>().ok())
            .unwrap_or(0)
            .min(15);
    }
    let weak = game.res.behaviours().facts(state).weak;
    ((weak >> (4 * from_reader.index() as u32)) & 15) as u8
}

/// The arms of one track: a same-arm track step, a vanilla wire step, or
/// a non-wire source at full strength.
fn track_supply(game: &crate::game::Game<'_>, pos: [i32; 3], shape: Shape) -> Vec<Supply> {
    let registry = &game.pack.assets.registry;
    let mut ends = vec![Supply::None; shape.arity()];
    for (index, dir) in shape.arms().iter().enumerate() {
        let neighbour_pos = dir.offset(pos);
        let Some(neighbour) = block_at(game, neighbour_pos) else {
            continue;
        };
        let Some(block) = registry.state(neighbour) else {
            continue;
        };
        let level = signal_toward(game, neighbour, *dir);
        if &*block.name == TRACK_BLOCK {
            // Only a track holding the arm that points back joins this arm.
            if track(registry, neighbour)
                .is_some_and(|other| connected(shape, pos, other.shape, neighbour_pos))
            {
                ends[index] = Supply::Track(level);
            }
        } else if &*block.name == WIRE_BLOCK {
            ends[index] = Supply::Wire(level);
        } else if level > 0 {
            ends[index] = Supply::Source(level);
        }
    }
    ends
}

/// Which of a track's arms reach a wired neighbour (a track pointing back, a
/// vanilla wire, or a powered source). The pen's bend gesture keeps the wired
/// arm, and two wired arms widen the net into a T (upstream's
/// `isAdjacentWireSegmentAddable`, `RedstoneTrack.java:934-944`).
fn wired_arms(game: &crate::game::Game<'_>, pos: [i32; 3], shape: Shape) -> Vec<bool> {
    let registry = &game.pack.assets.registry;
    let mut arms = vec![false; shape.arity()];
    for (index, dir) in shape.arms().iter().enumerate() {
        let neighbour_pos = dir.offset(pos);
        let Some(neighbour) = block_at(game, neighbour_pos) else {
            continue;
        };
        let Some(block) = registry.state(neighbour) else {
            continue;
        };
        arms[index] = if &*block.name == TRACK_BLOCK {
            track(registry, neighbour)
                .is_some_and(|other| connected(shape, pos, other.shape, neighbour_pos))
        } else if &*block.name == WIRE_BLOCK {
            true
        } else {
            signal_toward(game, neighbour, *dir) > 0
        };
    }
    arms
}

/// Whether the acting player sneaks, the erase gesture's modifier.
///
/// The pin's `NativeServerPlayer` carries no sneaking flag, so this reads the
/// local input the engine's own placement test uses (`game.rs:6492`); the
/// integrated server shares the process. On a dedicated server a guest's
/// sneak is therefore invisible — a recorded deviation.
fn sneaking(game: &crate::game::Game<'_>) -> bool {
    game.keys.contains(&winit::keyboard::KeyCode::ShiftLeft)
}

/// The five input sides of a relay: everything except its output face.
fn relay_levels(game: &crate::game::Game<'_>, pos: [i32; 3], output: Dir) -> [u8; 5] {
    let mut levels = [0u8; 5];
    let mut index = 0;
    for dir in Dir::ALL {
        if dir == output {
            continue;
        }
        let neighbour_pos = dir.offset(pos);
        if let Some(neighbour) = block_at(game, neighbour_pos) {
            levels[index] = signal_toward(game, neighbour, dir);
        }
        index += 1;
    }
    levels
}

/// `block_signal`: a track answers its stored power on its arms, a relay
/// 15 on its output face while powered. Both answer the direct (strong) query
/// like upstream's `getDirectSignal`; the block's own template never provides
/// a conducted answer because this hook owns the whole signal.
#[allow(private_interfaces)]
pub fn on_block_signal(
    level: &crate::game::GameLevel<'_>,
    pos: BlockPos,
    state: BlockStateId,
    face: GeoDir,
    direct: bool,
    signal: &mut i32,
) {
    let _ = (pos, direct);
    if let Some(track) = track(level.registry, state) {
        *signal = track_signal(track.power, track.shape, to_dir(face));
        return;
    }
    if let Some(relay) = relay(level.registry, state) {
        *signal = relay_variant_signal(relay.kind, relay.phase, relay.output, to_dir(face));
    }
}

/// `is_signal_source`: a track and a relay both answer the signal query.
#[allow(private_interfaces)]
pub fn on_is_signal_source(
    level: &crate::game::GameLevel<'_>,
    state: BlockStateId,
    source: &mut bool,
) {
    if track(level.registry, state).is_some() || relay(level.registry, state).is_some() {
        *source = true;
    }
}

/// A neighbour changed: queue the track or relay at `pos`.
#[allow(private_interfaces)]
pub fn on_neighbor_changed(
    level: &crate::game::GameLevel<'_>,
    dimension: LevelDimension,
    pos: BlockPos,
    state: BlockStateId,
) {
    let at = [pos.x, pos.y, pos.z];
    if track(level.registry, state).is_some() {
        mark_track(dimension_name(dimension), at);
    } else if relay(level.registry, state).is_some() {
        mark_relay(dimension_name(dimension), at);
    }
}

/// A block was placed/changed: seed the first evaluation and re-evaluate the
/// blocks around it. A state write of this port is not a placement.
#[allow(private_interfaces)]
pub fn on_block_changed(
    level: &crate::game::GameLevel<'_>,
    dimension: LevelDimension,
    pos: BlockPos,
    new: BlockStateId,
) {
    let dimension = dimension_name(dimension);
    let at = [pos.x, pos.y, pos.z];
    let key = (dimension.to_owned(), at);
    if state()
        .get_or_insert_with(State::default)
        .writing
        .remove(&key)
    {
        return;
    }
    if track(level.registry, new).is_some() {
        mark_track(dimension, at);
        mark_around_game(level, dimension, at);
    } else if relay(level.registry, new).is_some() {
        mark_relay(dimension, at);
        mark_around_game(level, dimension, at);
    } else {
        mark_around_game(level, dimension, at);
    }
}

/// Queue the tracks/relays around a changed block (the `&GameLevel` variant of
/// `mark_around`, used by the observe hooks).
fn mark_around_game(
    level: &crate::game::GameLevel<'_>,
    dimension: &str,
    pos: [i32; 3],
) {
    for dir in Dir::ALL {
        let neighbour_pos = dir.offset(pos);
        let Some(neighbour) = level_block(level, neighbour_pos) else {
            continue;
        };
        if track(level.registry, neighbour).is_some() {
            mark_track(dimension, neighbour_pos);
        } else if relay(level.registry, neighbour).is_some() {
            mark_relay(dimension, neighbour_pos);
        }
    }
}

/// `getStateForPlacement`: a track takes the axis the player draws along; a
/// relay outputs away from the support it was clicked on.
#[allow(private_interfaces)]
pub fn on_state_for_placement(
    level: &crate::game::GameLevel<'_>,
    state: BlockStateId,
    context: &PlacementContext,
    answer: &mut Option<Option<BlockStateId>>,
) {
    if track(level.registry, state).is_some() {
        let axis = placement_axis(to_dir(context.face), context.yaw, context.pitch);
        if let Some(new) = with_shape(level.registry, state, Shape::straight(axis)) {
            *answer = Some(Some(new));
        }
        return;
    }
    if relay(level.registry, state).is_some() {
        let output = relay_facing(to_dir(context.face));
        if let Some(new) = relay_state(level.registry, state, output, RelayPhase::default()) {
            *answer = Some(Some(new));
        }
    }
}

/// The look direction of the acting player, or the local view when the hook
/// carries no acting player.
fn look(game: &crate::game::Game<'_>) -> (f32, f32) {
    match game.native_acting_player() {
        Some(player) => (player.yaw, player.pitch),
        None => (game.yaw, game.pitch),
    }
}

/// `use_item_on`: a pen click on a support draws a straight track on the
/// clicked face; on a track it extends the clicked arm, bends a one-wired-arm
/// net into a corner, widens a two-wired-arm net into a T on a side click, or
/// erases. A sneak click is the erase modifier: it drops the clicked arm of a
/// T and otherwise removes the cell, without needing the neighbours cut first.
#[allow(private_interfaces)]
pub fn on_use_item_on(
    game: &mut crate::game::Game<'_>,
    stack: &horizon_client::items::ItemStack,
    pos: [i32; 3],
    face: [i32; 3],
    claimed: &mut bool,
) {
    if &*stack.item != PEN_ITEM {
        return;
    }
    let Some(face) = dir_from_vector(face) else {
        return;
    };
    let Some(clicked) = block_at(game, pos) else {
        return;
    };
    let mode = if sneaking(game) {
        ClickMode::Erase
    } else {
        ClickMode::Draw
    };
    let dimension = game
        .native_acting_player()
        .map_or_else(|| game.dim.id().to_owned(), |player| player.dimension.clone());
    let registry = &game.pack.assets.registry;
    if let Some(track_now) = track(registry, clicked) {
        let wired = wired_arms(game, pos, track_now.shape);
        match click_action(track_now.shape, face, &wired, mode) {
            ClickAction::Extend(dir) => {
                let target = dir.offset(pos);
                if !is_air(game, target) {
                    // The clicked arm's cell is occupied: consume the click.
                    *claimed = true;
                    return;
                }
                if let Some(state) = track_state(registry, Shape::straight(dir.axis()), 0)
                    && write(game, &dimension, target, state, true)
                {
                    mark_track(&dimension, target);
                    *claimed = true;
                }
            }
            ClickAction::Bend(shape) | ClickAction::Widen(shape) | ClickAction::Narrow(shape) => {
                if let Some(state) = with_shape(registry, clicked, shape)
                    && write(game, &dimension, pos, state, true)
                {
                    mark_track(&dimension, pos);
                    mark_around(game, &dimension, pos);
                    *claimed = true;
                }
            }
            ClickAction::Erase => {
                if let Some(air) = air_state(registry)
                    && write(game, &dimension, pos, air, true)
                {
                    // The neighbours have to drain: this write's own
                    // `block_changed` is swallowed by the `writing` sentinel.
                    mark_around(game, &dimension, pos);
                    *claimed = true;
                }
            }
            ClickAction::Refuse => *claimed = true,
        }
        return;
    }
    if mode == ClickMode::Erase {
        // The erase modifier never draws: a sneak click on a support is inert.
        *claimed = true;
        return;
    }
    let target = face.offset(pos);
    if !is_air(game, target) {
        return;
    }
    let (yaw, pitch) = look(game);
    let axis = placement_axis(face, yaw, pitch);
    let Some(state) = track_state(registry, Shape::straight(axis), 0) else {
        return;
    };
    if write(game, &dimension, target, state, true) {
        mark_track(&dimension, target);
        *claimed = true;
    }
}

/// Apply the queued delayed writes, relays and tracks for this dimension.
#[allow(private_interfaces)]
pub fn on_level_tick_post(game: &mut crate::game::Game<'_>, dimension: &str) {
    advance_pending(game, dimension);
    solve_relays(game, dimension);
    solve_tracks(game, dimension);
}

/// Advance upstream's scheduled block ticks. A pending write lands two level
/// ticks after it was armed, like `scheduleTick(pos, this, 2)`.
fn advance_pending(game: &mut crate::game::Game<'_>, dimension: &str) {
    let ready: Vec<((String, [i32; 3]), Pending)> = {
        let mut guard = state();
        let state = guard.get_or_insert_with(State::default);
        let keys: Vec<(String, [i32; 3])> = state
            .pending
            .keys()
            .filter(|(dim, _)| dim == dimension)
            .cloned()
            .collect();
        let mut ready = Vec::new();
        for key in keys {
            let Some(pending) = state.pending.get_mut(&key) else {
                continue;
            };
            if pending.ticks > 1 {
                pending.ticks -= 1;
            } else {
                let done = *pending;
                state.pending.remove(&key);
                ready.push((key, done));
            }
        }
        ready
    };
    for ((_, pos), pending) in ready {
        let Some(current) = block_at(game, pos) else {
            continue;
        };
        let Some(relay_now) = relay(&game.pack.assets.registry, current) else {
            continue;
        };
        if relay_now.kind != pending.kind {
            continue;
        }
        let phase = match pending.write {
            PendingWrite::Phase(phase) => phase,
            PendingWrite::PulseClear => RelayPhase {
                powered: relay_now.phase.powered,
                state: false,
            },
        };
        let Some(new) = relay_state(
            &game.pack.assets.registry,
            current,
            relay_now.output,
            phase,
        ) else {
            continue;
        };
        if write(game, dimension, pos, new, pending.notify) {
            mark_around(game, dimension, pos);
        }
    }
}

/// Re-evaluate the queued relays. A relay that switched marks its output
/// neighbour, so the first track past it re-strengthens in this same tick.
fn solve_relays(game: &mut crate::game::Game<'_>, dimension: &str) {
    let batch: Vec<[i32; 3]> = {
        let mut guard = state();
        let state = guard.get_or_insert_with(State::default);
        let ready: Vec<(String, [i32; 3])> = state
            .relays
            .iter()
            .filter(|(dim, _)| dim == dimension)
            .cloned()
            .collect();
        for key in &ready {
            state.relays.remove(key);
        }
        ready.into_iter().map(|(_, pos)| pos).collect()
    };
    for pos in batch {
        let Some(current) = block_at(game, pos) else {
            continue;
        };
        let Some(relay_now) = relay(&game.pack.assets.registry, current) else {
            continue;
        };
        let levels = relay_levels(game, pos, relay_now.output);
        let powered = relay_powered(&levels);
        let before = relay_now.phase;
        let Some(next) = relay_edge(relay_now.kind, powered, before) else {
            // A pulse may have been placed (or loaded) with STATE=1; make sure
            // its 2-tick clear is still scheduled.
            arm_pulse_clear(dimension, pos, relay_now.kind, before);
            continue;
        };
        if relay_delayed_off(relay_now.kind, powered, before) {
            // Upstream leaves the state alone and schedules a 2-tick tick.
            schedule_after(
                dimension,
                pos,
                RELAY_OFF_DELAY_TICKS,
                relay_now.kind,
                PendingWrite::Phase(next),
                true,
            );
            continue;
        }
        let Some(new) = relay_state(
            &game.pack.assets.registry,
            current,
            relay_now.output,
            next,
        ) else {
            continue;
        };
        let notify = relay_notifies(relay_now.kind, powered, before);
        if write(game, dimension, pos, new, notify) {
            mark_around(game, dimension, pos);
            arm_pulse_clear(dimension, pos, relay_now.kind, next);
        }
    }
}

/// Schedule a pulse relay's `STATE` clear. A pending tick is not re-armed
/// (`hasScheduledTick` upstream), so an earlier deadline wins.
fn arm_pulse_clear(dimension: &str, pos: [i32; 3], kind: RelayKind, phase: RelayPhase) {
    if kind == RelayKind::Pulse && phase.state {
        schedule_after(
            dimension,
            pos,
            PULSE_TICKS,
            kind,
            PendingWrite::PulseClear,
            true,
        );
    }
}

/// Arm one delayed write, keeping an earlier deadline for the same position.
fn schedule_after(
    dimension: &str,
    pos: [i32; 3],
    ticks: u8,
    kind: RelayKind,
    write: PendingWrite,
    notify: bool,
) {
    let key = (dimension.to_owned(), pos);
    let mut guard = state();
    let state = guard.get_or_insert_with(State::default);
    if state.pending.len() < MAX_DIRTY {
        state.pending.entry(key).or_insert(Pending {
            ticks,
            kind,
            write,
            notify,
        });
    }
}

/// Recompute the queued tracks and everything a change reaches, in one tick.
fn solve_tracks(game: &mut crate::game::Game<'_>, dimension: &str) {
    let mut queue: VecDeque<[i32; 3]> = {
        let mut guard = state();
        let state = guard.get_or_insert_with(State::default);
        let ready: Vec<(String, [i32; 3])> = state
            .tracks
            .iter()
            .filter(|(dim, _)| dim == dimension)
            .cloned()
            .collect();
        for key in &ready {
            state.tracks.remove(key);
        }
        ready.into_iter().map(|(_, pos)| pos).collect()
    };
    let mut visits: BTreeMap<[i32; 3], u8> = BTreeMap::new();
    let mut steps = MAX_STEPS;
    while let Some(pos) = queue.pop_front() {
        if steps == 0 {
            break;
        }
        let count = visits.entry(pos).or_insert(0);
        if *count >= MAX_VISITS {
            continue;
        }
        *count += 1;
        steps -= 1;
        let Some(current) = block_at(game, pos) else {
            continue;
        };
        let Some(track_now) = track(&game.pack.assets.registry, current) else {
            continue;
        };
        let ends = track_supply(game, pos, track_now.shape);
        let next = track_power(&ends);
        if next == track_now.power {
            continue;
        }
        let Some(new) = with_power(&game.pack.assets.registry, current, next) else {
            continue;
        };
        if !write(game, dimension, pos, new, true) {
            continue;
        }
        // Every track on one of the arms, and any relay reading this one.
        for dir in Dir::ALL {
            let neighbour_pos = dir.offset(pos);
            let Some(neighbour) = block_at(game, neighbour_pos) else {
                continue;
            };
            let registry = &game.pack.assets.registry;
            if let Some(neighbour_track) = track(registry, neighbour) {
                if connected(track_now.shape, pos, neighbour_track.shape, neighbour_pos) {
                    queue.push_back(neighbour_pos);
                }
            } else if relay(registry, neighbour).is_some() {
                mark_relay(dimension, neighbour_pos);
            }
        }
    }
}

/// Write `new` at `pos` and notify its neighbours. The `writing` sentinel
/// hides the engine's `block_changed` self-report from `on_block_changed`.
fn write(
    game: &mut crate::game::Game<'_>,
    dimension: &str,
    pos: [i32; 3],
    new: BlockStateId,
    notify: bool,
) -> bool {
    let key = (dimension.to_owned(), pos);
    state()
        .get_or_insert_with(State::default)
        .writing
        .insert(key.clone());
    let wrote = game.native_set_block(dimension, pos, new, flags::UPDATE_ALL);
    // Also drop a sentinel the hook never consumed (a non-authoritative level,
    // or a refused write), so it cannot swallow a later placement.
    state()
        .get_or_insert_with(State::default)
        .writing
        .remove(&key);
    if wrote && notify {
        game.native_update_signal_neighbors(dimension, pos);
    }
    wrote
}
