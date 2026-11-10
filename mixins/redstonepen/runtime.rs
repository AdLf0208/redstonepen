// Redstone Pen for Horizon — the two-or-three-arm track model (pure).
//
// Ported from Redstone Pen 1.11.42 (stfwi/redstonepen, MIT) for Minecraft Java
// 1.21.1 NeoForge. Upstream source read for behaviour only; this is a
// clean-room Rust rewrite.
//
// Upstream packs up to 24 independent wire segments into one block's block
// entity (four in-plane directions on each of six faces) plus six face
// connectors. A block state cannot express that, so this slice is the
// documented simplification: one track per block position with **two or
// three arms** and one `power` value 0..15. Two opposite arms are a straight
// track (an `axis`), two perpendicular arms are a corner, and three arms are
// a T — the largest net the state model holds. Multiple tracks, centre
// connectors and per-face attachment stay out of scope.
//
// The signal model keeps upstream's rules: a track answers its power on all
// its arms, a track-to-track step costs one, a vanilla redstone wire costs
// one, a non-wire source (a redstone block, a lever, a relay) seeds its full
// strength, and a relay re-emits 15 from one output face.

/// The six faces in Minecraft's `Direction` ordinal order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dir {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Dir {
    /// The six faces in ordinal order.
    pub const ALL: [Dir; 6] = [
        Dir::Down,
        Dir::Up,
        Dir::North,
        Dir::South,
        Dir::West,
        Dir::East,
    ];

    pub fn opposite(self) -> Dir {
        match self {
            Dir::Down => Dir::Up,
            Dir::Up => Dir::Down,
            Dir::North => Dir::South,
            Dir::South => Dir::North,
            Dir::West => Dir::East,
            Dir::East => Dir::West,
        }
    }

    /// The `Direction` ordinal (Down 0 … East 5).
    pub fn index(self) -> usize {
        match self {
            Dir::Down => 0,
            Dir::Up => 1,
            Dir::North => 2,
            Dir::South => 3,
            Dir::West => 4,
            Dir::East => 5,
        }
    }

    pub fn axis(self) -> Axis {
        match self {
            Dir::Down | Dir::Up => Axis::Y,
            Dir::North | Dir::South => Axis::Z,
            Dir::West | Dir::East => Axis::X,
        }
    }

    pub fn offset(self, [x, y, z]: [i32; 3]) -> [i32; 3] {
        match self {
            Dir::Down => [x, y - 1, z],
            Dir::Up => [x, y + 1, z],
            Dir::North => [x, y, z - 1],
            Dir::South => [x, y, z + 1],
            Dir::West => [x - 1, y, z],
            Dir::East => [x + 1, y, z],
        }
    }

    pub fn from_name(name: &str) -> Option<Dir> {
        Some(match name {
            "down" => Dir::Down,
            "up" => Dir::Up,
            "north" => Dir::North,
            "south" => Dir::South,
            "west" => Dir::West,
            "east" => Dir::East,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Dir::Down => "down",
            Dir::Up => "up",
            Dir::North => "north",
            Dir::South => "south",
            Dir::West => "west",
            Dir::East => "east",
        }
    }
}

/// A straight track's axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub fn from_name(name: &str) -> Option<Axis> {
        Some(match name {
            "x" => Axis::X,
            "y" => Axis::Y,
            "z" => Axis::Z,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Axis::X => "x",
            Axis::Y => "y",
            Axis::Z => "z",
        }
    }

    /// The two opposite faces a track runs between.
    pub fn directions(self) -> [Dir; 2] {
        match self {
            Axis::X => [Dir::West, Dir::East],
            Axis::Y => [Dir::Down, Dir::Up],
            Axis::Z => [Dir::North, Dir::South],
        }
    }
}

/// A track's net: the two or three faces its arms reach, at one power.
///
/// Upstream joins wire segments into nets through a shared face or an internal
/// edge (`RedstoneTrack.java:145-160`, `:1165-1180`, `:1208-1316`) and copies
/// the net's one power to every internal side (`:1103-1106`). A ported track
/// is that net reduced to its arms: two opposite arms are a straight track,
/// two perpendicular arms a corner, and three distinct arms a T. Three arms is
/// the largest net a block state can hold here: a fourth would need C(6,4)=15
/// more bend values and 2304 states, over the model's budget. The arms are
/// canonically ordered by [`Dir::index`], so a set has exactly one `Shape`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Shape {
    /// The arms, padded with a copy of the last one when the net has two.
    arms: [Dir; 3],
    arity: Arity,
}

/// How many arms a [`Shape`] reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Arity {
    Two,
    Three,
}

impl Arity {
    fn len(self) -> usize {
        match self {
            Arity::Two => 2,
            Arity::Three => 3,
        }
    }
}

impl Shape {
    /// A straight track between the two faces of `axis`.
    pub fn straight(axis: Axis) -> Shape {
        let [a, b] = axis.directions();
        Self::pair(a, b).expect("an axis names two distinct faces")
    }

    /// Two perpendicular arms, or `None` for faces that are the same or
    /// opposite (a corner needs a real 90° turn).
    pub fn corner(a: Dir, b: Dir) -> Option<Shape> {
        if a == b || a.opposite() == b {
            return None;
        }
        Self::pair(a, b)
    }

    /// Three distinct arms: a T. `None` when two of the faces coincide.
    pub fn tee(a: Dir, b: Dir, c: Dir) -> Option<Shape> {
        let mut arms = [a, b, c];
        arms.sort_by_key(|dir| dir.index());
        if arms[0] == arms[1] || arms[1] == arms[2] {
            return None;
        }
        Some(Shape {
            arms,
            arity: Arity::Three,
        })
    }

    /// Two distinct faces, canonically ordered, padded for the fixed array.
    fn pair(a: Dir, b: Dir) -> Option<Shape> {
        if a == b {
            return None;
        }
        let arms = if a.index() < b.index() {
            [a, b, b]
        } else {
            [b, a, a]
        };
        Some(Shape {
            arms,
            arity: Arity::Two,
        })
    }

    /// The arms in canonical order, one entry per face the net reaches.
    pub fn arms(&self) -> &[Dir] {
        &self.arms[..self.arity.len()]
    }

    /// How many arms the net has: two, or three for a T.
    pub fn arity(&self) -> usize {
        self.arity.len()
    }

    /// Whether the track answers on `dir` (the face toward the reader).
    pub fn has_arm(&self, dir: Dir) -> bool {
        self.arms().contains(&dir)
    }

    /// The `bend` property value of this shape: `none` for a straight track,
    /// the arms' names for a corner (`north_east`) or a T
    /// (`south_west_east`).
    pub fn bend_name(&self) -> &'static str {
        match *self.arms() {
            [a, b] if a.opposite() == b => "none",
            [a, b] => corner_name(a, b),
            [a, b, c] => tee_name(a, b, c),
            _ => "none",
        }
    }

    /// The shape a track state declares: its `axis` and `bend` properties.
    pub fn from_bend(axis: Axis, bend: &str) -> Option<Shape> {
        if bend == "none" {
            return Some(Shape::straight(axis));
        }
        if let Some((a, b)) = corner_from_name(bend) {
            return Shape::corner(a, b);
        }
        let (a, b, c) = tee_from_name(bend)?;
        Shape::tee(a, b, c)
    }

    /// The state's `axis` value: the axis of the first arm in canonical order,
    /// which is the bar the content's approximate collision box follows.
    pub fn axis(&self) -> Axis {
        self.arms()[0].axis()
    }
}

/// The twelve corner property names, by canonical arm pair.
const CORNER_NAMES: [(&str, Dir, Dir); 12] = [
    ("down_north", Dir::Down, Dir::North),
    ("down_south", Dir::Down, Dir::South),
    ("down_west", Dir::Down, Dir::West),
    ("down_east", Dir::Down, Dir::East),
    ("up_north", Dir::Up, Dir::North),
    ("up_south", Dir::Up, Dir::South),
    ("up_west", Dir::Up, Dir::West),
    ("up_east", Dir::Up, Dir::East),
    ("north_west", Dir::North, Dir::West),
    ("north_east", Dir::North, Dir::East),
    ("south_west", Dir::South, Dir::West),
    ("south_east", Dir::South, Dir::East),
];

/// The twenty T property names, by canonical arm triple (index order).
const TEE_NAMES: [(&str, Dir, Dir, Dir); 20] = [
    ("down_up_north", Dir::Down, Dir::Up, Dir::North),
    ("down_up_south", Dir::Down, Dir::Up, Dir::South),
    ("down_up_west", Dir::Down, Dir::Up, Dir::West),
    ("down_up_east", Dir::Down, Dir::Up, Dir::East),
    ("down_north_south", Dir::Down, Dir::North, Dir::South),
    ("down_north_west", Dir::Down, Dir::North, Dir::West),
    ("down_north_east", Dir::Down, Dir::North, Dir::East),
    ("down_south_west", Dir::Down, Dir::South, Dir::West),
    ("down_south_east", Dir::Down, Dir::South, Dir::East),
    ("down_west_east", Dir::Down, Dir::West, Dir::East),
    ("up_north_south", Dir::Up, Dir::North, Dir::South),
    ("up_north_west", Dir::Up, Dir::North, Dir::West),
    ("up_north_east", Dir::Up, Dir::North, Dir::East),
    ("up_south_west", Dir::Up, Dir::South, Dir::West),
    ("up_south_east", Dir::Up, Dir::South, Dir::East),
    ("up_west_east", Dir::Up, Dir::West, Dir::East),
    ("north_south_west", Dir::North, Dir::South, Dir::West),
    ("north_south_east", Dir::North, Dir::South, Dir::East),
    ("north_west_east", Dir::North, Dir::West, Dir::East),
    ("south_west_east", Dir::South, Dir::West, Dir::East),
];

fn corner_name(a: Dir, b: Dir) -> &'static str {
    CORNER_NAMES
        .iter()
        .find(|(_, ca, cb)| (*ca == a && *cb == b) || (*ca == b && *cb == a))
        .map_or("none", |(name, _, _)| name)
}

fn corner_from_name(name: &str) -> Option<(Dir, Dir)> {
    CORNER_NAMES
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, a, b)| (*a, *b))
}

fn tee_name(a: Dir, b: Dir, c: Dir) -> &'static str {
    TEE_NAMES
        .iter()
        .find(|(_, ta, tb, tc)| (*ta, *tb, *tc) == (a, b, c))
        .map_or("none", |(name, _, _, _)| name)
}

fn tee_from_name(name: &str) -> Option<(Dir, Dir, Dir)> {
    TEE_NAMES
        .iter()
        .find(|(n, _, _, _)| *n == name)
        .map(|(_, a, b, c)| (*a, *b, *c))
}

/// The gesture a pen click carries: a plain click draws, a sneak click erases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickMode {
    Draw,
    Erase,
}

/// What a pen click on an existing track does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickAction {
    /// The clicked face is an arm: add a straight track in that block.
    Extend(Dir),
    /// The clicked side becomes the second arm, keeping the arm that is
    /// already wired to a neighbour and dropping the unwired one.
    Bend(Shape),
    /// Both arms are wired: the clicked side becomes a third arm (a T).
    Widen(Shape),
    /// A sneak click on a T's arm drops that arm and keeps the other two.
    Narrow(Shape),
    /// Remove the whole track cell.
    Erase,
    /// Not representable (a fourth arm, or a U-turn): consume the click and
    /// change nothing.
    Refuse,
}

/// What a click on `face` does to a track, given which arms reach a wired
/// neighbour (a track that points back, a vanilla wire, or a signal source).
///
/// Upstream's pen adds the clicked in-plane segment and the neighbouring ones
/// that already have a wire pointing back (`RedstoneTrack.java:838-857`,
/// `:934-944`), which is how a drawn line grows a corner. This port reduces
/// the gesture to the nets it can express: a side click bends a one-wired-arm
/// net, widens a two-wired-arm net into a T, and a sneak click is upstream's
/// separate remove gesture (`:473-477`, `RedstonePenItem.java:227-245`):
/// it drops the clicked arm of a T, or the whole cell.
pub fn click_action(shape: Shape, face: Dir, wired: &[bool], mode: ClickMode) -> ClickAction {
    if mode == ClickMode::Erase {
        return erase_action(shape, face);
    }
    if shape.has_arm(face) {
        return ClickAction::Extend(face);
    }
    let live = wired.iter().filter(|wired| **wired).count();
    if live == 0 {
        return ClickAction::Erase;
    }
    if shape.arity() == 3 {
        return ClickAction::Refuse;
    }
    let Some(kept) = shape
        .arms()
        .iter()
        .copied()
        .enumerate()
        .find(|(index, _)| wired.get(*index) == Some(&true))
        .map(|(_, dir)| dir)
    else {
        return ClickAction::Erase;
    };
    if live == 1 {
        // A click on the back of the kept arm would straighten the track
        // across the block; that is not the bend gesture.
        if face == kept.opposite() {
            return ClickAction::Refuse;
        }
        return Shape::pair(kept, face).map_or(ClickAction::Refuse, ClickAction::Bend);
    }
    match *shape.arms() {
        [a, b] => Shape::tee(a, b, face).map_or(ClickAction::Refuse, ClickAction::Widen),
        _ => ClickAction::Refuse,
    }
}

/// A sneak click: a T loses just the clicked arm, anything else loses the
/// cell. Upstream's remove gesture drops the clicked segment too.
fn erase_action(shape: Shape, face: Dir) -> ClickAction {
    let narrowed = match *shape.arms() {
        [a, b, c] => drop_arm([a, b, c], face),
        _ => None,
    };
    narrowed.map_or(ClickAction::Erase, ClickAction::Narrow)
}

fn drop_arm([a, b, c]: [Dir; 3], face: Dir) -> Option<Shape> {
    if face == a {
        Shape::pair(b, c)
    } else if face == b {
        Shape::pair(a, c)
    } else if face == c {
        Shape::pair(a, b)
    } else {
        None
    }
}

/// What a track's end sees in the block beside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Supply {
    /// Air, an unpowered block, or a track on another axis.
    None,
    /// Another track: a step costs one (`RedstoneTrack.handleNetNeighborChanged`).
    Track(u8),
    /// A vanilla redstone wire: a step costs one.
    Wire(u8),
    /// Any other signal source: its full strength, no step cost.
    Source(u8),
}

/// The strength one end contributes to the track it feeds.
pub fn supply_power(supply: Supply) -> u8 {
    match supply {
        Supply::None => 0,
        Supply::Track(power) | Supply::Wire(power) => power.min(15).saturating_sub(1),
        Supply::Source(power) => power.min(15),
    }
}

/// A track's power: the strongest arm, after each arm's step cost. A T is one
/// net, so its three arms carry this one value (`RedstoneTrack.java:1098-1106`).
pub fn track_power(ends: &[Supply]) -> u8 {
    ends.iter().copied().map(supply_power).max().unwrap_or(0)
}

/// The power a track answers on `travel` (the signal's source-to-reader face).
///
/// A net answers on every one of its arms (`RedstoneTrack.java:1103-1106`),
/// so a corner answers on both of its faces, a straight track on its axis and
/// a T on all three arms.
pub fn track_signal(power: u8, shape: Shape, travel: Dir) -> i32 {
    if shape.has_arm(travel) {
        i32::from(power.min(15))
    } else {
        0
    }
}

/// The strongest of a relay's input sides.
pub fn relay_input(inputs: &[u8]) -> u8 {
    inputs.iter().copied().map(|p| p.min(15)).max().unwrap_or(0)
}

/// A relay is powered while any input side carries a signal.
pub fn relay_powered(inputs: &[u8]) -> bool {
    relay_input(inputs) > 0
}

/// The relay variants upstream's `CircuitComponents` registers; the plain
/// relay is the one slice 1 already shipped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RelayKind {
    Plain,
    Inverted,
    Pulse,
    Bistable,
}

impl RelayKind {
    pub const ALL: [RelayKind; 4] = [
        RelayKind::Plain,
        RelayKind::Inverted,
        RelayKind::Pulse,
        RelayKind::Bistable,
    ];

    /// The block id each variant is registered under (`ModContent.java:59-90`).
    pub fn block_id(self) -> &'static str {
        match self {
            RelayKind::Plain => "redstonepen:relay",
            RelayKind::Inverted => "redstonepen:inverted_relay",
            RelayKind::Pulse => "redstonepen:pulse_relay",
            RelayKind::Bistable => "redstonepen:bistable_relay",
        }
    }

    pub fn from_block_id(name: &str) -> Option<RelayKind> {
        RelayKind::ALL
            .into_iter()
            .find(|kind| kind.block_id() == name)
    }

    /// Whether the variant carries upstream's `STATE` latch (0/1). The plain
    /// and inverted relays only have `POWERED` (`CircuitComponents.java:552`).
    pub fn has_state(self) -> bool {
        matches!(self, RelayKind::Pulse | RelayKind::Bistable)
    }
}

/// A relay's stored phase: `powered` is upstream's input edge detector and
/// `state` is the variant latch (`STATE`, used by pulse and bistable).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RelayPhase {
    pub powered: bool,
    pub state: bool,
}

/// Upstream schedules a 2-tick block tick when a plain or inverted relay has
/// to turn its output off (`CircuitComponents.java:500-503`, `:535-537`).
pub const RELAY_OFF_DELAY_TICKS: u8 = 2;

/// The pulse relay clears its `STATE` two ticks after the rising edge
/// (`CircuitComponents.java:597-628`).
pub const PULSE_TICKS: u8 = 2;

/// The boolean the variant drives on its output face (`getSignal`).
pub fn relay_output(kind: RelayKind, phase: RelayPhase) -> bool {
    match kind {
        // RelayBlock: POWERED (`:474-479`).
        RelayKind::Plain => phase.powered,
        // InvertedRelayBlock: !POWERED (`:518-520`).
        RelayKind::Inverted => !phase.powered,
        // PulseRelayBlock / BistableRelayBlock: STATE (`:558-560`, `:593-595`).
        RelayKind::Pulse | RelayKind::Bistable => phase.state,
    }
}

/// The 0/15 signal a variant answers on `travel`, its output face only.
pub fn relay_variant_signal(kind: RelayKind, phase: RelayPhase, output: Dir, travel: Dir) -> i32 {
    if travel == output && relay_output(kind, phase) {
        15
    } else {
        0
    }
}

/// The phase a variant moves to on an input edge, or `None` while the input
/// matches the stored edge detector.
///
/// - bistable toggles `state` on a rising edge and holds it when the input
///   falls (`CircuitComponents.java:562-584`);
/// - pulse sets `state` on a rising edge; the scheduled tick clears it
///   (`:593-628`);
/// - plain and inverted track the input in `powered` (`:482-505`, `:522-549`).
pub fn relay_edge(kind: RelayKind, input_powered: bool, phase: RelayPhase) -> Option<RelayPhase> {
    if input_powered == phase.powered {
        return None;
    }
    Some(match kind {
        RelayKind::Plain | RelayKind::Inverted => RelayPhase {
            powered: input_powered,
            state: false,
        },
        RelayKind::Pulse => RelayPhase {
            powered: input_powered,
            state: input_powered || phase.state,
        },
        RelayKind::Bistable => RelayPhase {
            powered: input_powered,
            state: if input_powered {
                !phase.state
            } else {
                phase.state
            },
        },
    })
}

/// Whether this edge has to wait `RELAY_OFF_DELAY_TICKS` because it turns the
/// output off: a plain relay's falling input, an inverted relay's rising input.
pub fn relay_delayed_off(kind: RelayKind, input_powered: bool, phase: RelayPhase) -> bool {
    match kind {
        RelayKind::Plain => !input_powered && phase.powered,
        RelayKind::Inverted => input_powered && !phase.powered,
        RelayKind::Pulse | RelayKind::Bistable => false,
    }
}

/// Whether an immediate edge write must notify the output face. Bistable and
/// pulse only notify when their latched output turns on; the inverted relay's
/// delayed turn-off is notified when the delayed write lands.
pub fn relay_notifies(kind: RelayKind, input_powered: bool, before: RelayPhase) -> bool {
    match kind {
        RelayKind::Plain => input_powered,
        RelayKind::Inverted => !input_powered,
        RelayKind::Pulse => input_powered && !before.state,
        RelayKind::Bistable => input_powered,
    }
}

/// The axis a track gets when drawn on `face` (the clicked face).
///
/// The look vector is projected onto the face's plane and the dominant
/// in-plane axis wins; a look straight along the face normal falls back to the
/// horizontal axis of that plane. `yaw`/`pitch` are degrees, Minecraft's
/// convention (yaw 0 looks south, pitch 90 looks down).
pub fn placement_axis(face: Dir, yaw: f32, pitch: f32) -> Axis {
    let yaw = yaw.to_radians();
    let pitch = pitch.to_radians();
    // Minecraft: yaw 0 looks south (+z), 90 west (-x); pitch 90 looks down.
    let look = [-yaw.sin(), -pitch.sin(), yaw.cos()];
    let normal = match face {
        Dir::Down => [0.0, -1.0, 0.0],
        Dir::Up => [0.0, 1.0, 0.0],
        Dir::North => [0.0, 0.0, -1.0],
        Dir::South => [0.0, 0.0, 1.0],
        Dir::West => [-1.0, 0.0, 0.0],
        Dir::East => [1.0, 0.0, 0.0],
    };
    let along = look[0] * normal[0] + look[1] * normal[1] + look[2] * normal[2];
    let projected = [
        look[0] - along * normal[0],
        look[1] - along * normal[1],
        look[2] - along * normal[2],
    ];
    // The first two axes of the face's plane, and the horizontal default when
    // the look runs straight along the face normal.
    let (first, second, first_axis, second_axis, fallback) = match face.axis() {
        Axis::Y => (
            projected[0].abs(),
            projected[2].abs(),
            Axis::X,
            Axis::Z,
            Axis::Z,
        ),
        Axis::Z => (
            projected[0].abs(),
            projected[1].abs(),
            Axis::X,
            Axis::Y,
            Axis::X,
        ),
        Axis::X => (
            projected[2].abs(),
            projected[1].abs(),
            Axis::Z,
            Axis::Y,
            Axis::Z,
        ),
    };
    const EPSILON: f32 = 1e-4;
    if first <= EPSILON && second <= EPSILON {
        fallback
    } else if first > second {
        first_axis
    } else {
        second_axis
    }
}

/// Whether two adjacent tracks join: `other` sits on one of `shape`'s arms
/// and `other_shape` holds the arm that points back.
///
/// Upstream only joins a segment to a neighbouring track whose matching
/// segment points the other way (`RedstoneTrack.java:1230-1246`).
pub fn connected(shape: Shape, pos: [i32; 3], other_shape: Shape, other: [i32; 3]) -> bool {
    shape
        .arms()
        .iter()
        .copied()
        .any(|arm| arm.offset(pos) == other && other_shape.has_arm(arm.opposite()))
}

/// The output face of a relay placed against `support_face`.
pub fn relay_facing(support_face: Dir) -> Dir {
    support_face.opposite()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opposite_faces_are_involutive() {
        for dir in Dir::ALL {
            assert_eq!(dir.opposite().opposite(), dir);
            assert_ne!(dir.opposite(), dir);
        }
        assert_eq!(Dir::Down.opposite(), Dir::Up);
        assert_eq!(Dir::North.opposite(), Dir::South);
        assert_eq!(Dir::West.opposite(), Dir::East);
    }

    #[test]
    fn faces_keep_the_minecraft_direction_order() {
        assert_eq!(Dir::Down.index(), 0);
        assert_eq!(Dir::Up.index(), 1);
        assert_eq!(Dir::North.index(), 2);
        assert_eq!(Dir::South.index(), 3);
        assert_eq!(Dir::West.index(), 4);
        assert_eq!(Dir::East.index(), 5);
    }

    #[test]
    fn each_face_belongs_to_one_axis() {
        assert_eq!(Dir::Down.axis(), Axis::Y);
        assert_eq!(Dir::Up.axis(), Axis::Y);
        assert_eq!(Dir::North.axis(), Axis::Z);
        assert_eq!(Dir::South.axis(), Axis::Z);
        assert_eq!(Dir::West.axis(), Axis::X);
        assert_eq!(Dir::East.axis(), Axis::X);
    }

    #[test]
    fn an_axis_names_its_two_ends() {
        assert_eq!(Axis::X.directions(), [Dir::West, Dir::East]);
        assert_eq!(Axis::Y.directions(), [Dir::Down, Dir::Up]);
        assert_eq!(Axis::Z.directions(), [Dir::North, Dir::South]);
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            for dir in axis.directions() {
                assert_eq!(dir.axis(), axis);
            }
        }
    }

    #[test]
    fn names_round_trip() {
        for dir in Dir::ALL {
            assert_eq!(Dir::from_name(dir.name()), Some(dir));
        }
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            assert_eq!(Axis::from_name(axis.name()), Some(axis));
        }
        assert_eq!(Dir::from_name("sideways"), None);
        assert_eq!(Axis::from_name("w"), None);
    }

    #[test]
    fn offsets_move_by_one_face() {
        let origin = [4, 64, -2];
        assert_eq!(Dir::Down.offset(origin), [4, 63, -2]);
        assert_eq!(Dir::Up.offset(origin), [4, 65, -2]);
        assert_eq!(Dir::North.offset(origin), [4, 64, -3]);
        assert_eq!(Dir::South.offset(origin), [4, 64, -1]);
        assert_eq!(Dir::West.offset(origin), [3, 64, -2]);
        assert_eq!(Dir::East.offset(origin), [5, 64, -2]);
    }

    #[test]
    fn a_source_seeds_its_strength_and_a_track_step_costs_one() {
        assert_eq!(supply_power(Supply::None), 0);
        assert_eq!(supply_power(Supply::Source(15)), 15);
        assert_eq!(supply_power(Supply::Source(9)), 9);
        assert_eq!(supply_power(Supply::Track(15)), 14);
        assert_eq!(supply_power(Supply::Wire(15)), 14);
        assert_eq!(supply_power(Supply::Track(0)), 0);
        assert_eq!(supply_power(Supply::Wire(1)), 0);
    }

    #[test]
    fn a_track_takes_the_strongest_end() {
        assert_eq!(track_power(&[Supply::None, Supply::None]), 0);
        assert_eq!(track_power(&[Supply::Source(15), Supply::None]), 15);
        // A source on one end beats a decayed track on the other.
        assert_eq!(track_power(&[Supply::Track(3), Supply::Source(15)]), 15);
        // Two fed track ends: the stronger survives after its step cost.
        assert_eq!(track_power(&[Supply::Track(15), Supply::Track(12)]), 14);
        assert_eq!(track_power(&[Supply::Wire(9), Supply::Track(4)]), 8);
        assert_eq!(track_power(&[]), 0);
    }

    #[test]
    fn a_tee_takes_the_strongest_of_its_three_arms() {
        let tee = Shape::tee(Dir::West, Dir::East, Dir::South).expect("a T");
        let ends = [Supply::Track(15), Supply::Track(12), Supply::Wire(4)];
        assert_eq!(track_power(&ends), 14);
        // A source on the branch seeds full strength; the other arms decay from it.
        let ends = [Supply::Track(15), Supply::Source(15), Supply::None];
        assert_eq!(track_power(&ends), 15);
        assert_eq!(track_power(&[Supply::None; 3]), 0);
        // The three-arm net copies one power to every arm (upstream `:1103-1106`).
        for arm in tee.arms() {
            assert_eq!(track_signal(13, tee, *arm), 13, "{}", arm.name());
        }
    }

    #[test]
    fn a_track_answers_only_along_its_arms() {
        let x = Shape::straight(Axis::X);
        assert_eq!(track_signal(11, x, Dir::East), 11);
        assert_eq!(track_signal(11, x, Dir::West), 11);
        assert_eq!(track_signal(11, x, Dir::North), 0);
        assert_eq!(track_signal(11, x, Dir::Up), 0);
        let z = Shape::straight(Axis::Z);
        assert_eq!(track_signal(7, z, Dir::South), 7);
        assert_eq!(track_signal(7, z, Dir::East), 0);
        let y = Shape::straight(Axis::Y);
        assert_eq!(track_signal(20, y, Dir::Down), 15);
        assert_eq!(track_signal(0, y, Dir::Up), 0);
    }

    #[test]
    fn a_relay_re_emits_15_only_from_its_output_face() {
        let on = RelayPhase {
            powered: true,
            state: false,
        };
        let off = RelayPhase::default();
        assert_eq!(
            relay_variant_signal(RelayKind::Plain, on, Dir::East, Dir::East),
            15
        );
        assert_eq!(
            relay_variant_signal(RelayKind::Plain, on, Dir::East, Dir::West),
            0
        );
        assert_eq!(
            relay_variant_signal(RelayKind::Plain, on, Dir::East, Dir::Up),
            0
        );
        assert_eq!(
            relay_variant_signal(RelayKind::Plain, off, Dir::East, Dir::East),
            0
        );
    }

    #[test]
    fn a_relay_is_powered_by_any_input_side() {
        assert_eq!(relay_input(&[]), 0);
        assert_eq!(relay_input(&[0, 0, 0, 0, 0]), 0);
        assert_eq!(relay_input(&[0, 7, 0, 3, 0]), 7);
        assert!(!relay_powered(&[0, 0, 0, 0, 0]));
        assert!(relay_powered(&[0, 0, 1, 0, 0]));
        assert!(relay_powered(&[15]));
    }

    #[test]
    fn a_signal_decays_to_zero_after_fifteen_track_steps() {
        // A redstone block seeds 15; each track step costs one, so track n
        // (1-based) carries 15 - (n - 1) and track 16 is dark.
        let mut supply = Supply::Source(15);
        for step in 1..=15 {
            let power = supply_power(supply);
            assert_eq!(power, 16 - step, "track {step}");
            supply = Supply::Track(power);
        }
        assert_eq!(supply_power(supply), 0, "the 16th step is dark");
    }

    #[test]
    fn floor_tracks_follow_the_horizontal_look() {
        // yaw 0 looks south (+z), 90 west (-x), 180 north, 270 east (+x).
        assert_eq!(placement_axis(Dir::Up, 0.0, 45.0), Axis::Z);
        assert_eq!(placement_axis(Dir::Up, 90.0, 45.0), Axis::X);
        assert_eq!(placement_axis(Dir::Up, 180.0, 45.0), Axis::Z);
        assert_eq!(placement_axis(Dir::Up, 270.0, 45.0), Axis::X);
        // Looking straight down gives no in-plane direction: the default z.
        assert_eq!(placement_axis(Dir::Up, 0.0, 90.0), Axis::Z);
        // The same rule applies to the underside.
        assert_eq!(placement_axis(Dir::Down, 90.0, -45.0), Axis::X);
    }

    #[test]
    fn wall_tracks_pick_an_axis_inside_the_wall_plane() {
        // Looking straight at a south wall: default horizontal x.
        assert_eq!(placement_axis(Dir::South, 0.0, 0.0), Axis::X);
        // Looking along the wall westwards: x.
        assert_eq!(placement_axis(Dir::South, 90.0, 0.0), Axis::X);
        // Looking down the wall: y.
        assert_eq!(placement_axis(Dir::South, 0.0, 80.0), Axis::Y);
        // An east/west wall defaults to z.
        assert_eq!(placement_axis(Dir::East, 0.0, 0.0), Axis::Z);
        // Facing the west wall (yaw 90) and looking up it: y.
        assert_eq!(placement_axis(Dir::West, 90.0, -80.0), Axis::Y);
        // A north wall: the player looks north (-z), so z is the normal.
        assert_eq!(placement_axis(Dir::North, 180.0, 0.0), Axis::X);
    }

    #[test]
    fn an_axis_click_extends_a_straight_track_and_a_side_click_erases_it() {
        let x = Shape::straight(Axis::X);
        assert_eq!(
            click_action(x, Dir::East, &[false, false], ClickMode::Draw),
            ClickAction::Extend(Dir::East)
        );
        assert_eq!(
            click_action(x, Dir::West, &[false, false], ClickMode::Draw),
            ClickAction::Extend(Dir::West)
        );
        assert_eq!(
            click_action(x, Dir::Up, &[false, false], ClickMode::Draw),
            ClickAction::Erase
        );
        let y = Shape::straight(Axis::Y);
        assert_eq!(
            click_action(y, Dir::Up, &[false, false], ClickMode::Draw),
            ClickAction::Extend(Dir::Up)
        );
        assert_eq!(
            click_action(y, Dir::North, &[false, false], ClickMode::Draw),
            ClickAction::Erase
        );
        let z = Shape::straight(Axis::Z);
        assert_eq!(
            click_action(z, Dir::North, &[false, false], ClickMode::Draw),
            ClickAction::Extend(Dir::North)
        );
        assert_eq!(
            click_action(z, Dir::Down, &[false, false], ClickMode::Draw),
            ClickAction::Erase
        );
    }

    #[test]
    fn straight_tracks_connect_only_on_the_same_axis_immediately_beside_each_other() {
        let here = [0, 64, 0];
        let x = Shape::straight(Axis::X);
        let y = Shape::straight(Axis::Y);
        let z = Shape::straight(Axis::Z);
        assert!(connected(x, here, x, [1, 64, 0]));
        assert!(connected(x, here, x, [-1, 64, 0]));
        assert!(!connected(x, here, z, [0, 64, 1]));
        assert!(!connected(x, here, x, [2, 64, 0]));
        assert!(!connected(x, here, x, [1, 65, 0]));
        assert!(!connected(x, here, x, [1, 64, 1]));
        assert!(connected(y, here, y, [0, 65, 0]));
        assert!(connected(z, here, z, [0, 64, -1]));
        // A track on the right side but across its own axis does not join.
        assert!(!connected(z, here, x, [0, 64, -1]));
    }

    #[test]
    fn corner_shapes_name_their_two_arms() {
        for (name, a, b) in CORNER_NAMES {
            let shape = Shape::corner(a, b).expect("a perpendicular pair is a corner");
            assert_eq!(shape, Shape::corner(b, a).expect("a corner is unordered"));
            let wanted = if a.index() < b.index() {
                [a, b]
            } else {
                [b, a]
            };
            assert_eq!(shape.arms(), wanted, "{name} keeps canonical arm order");
            assert!(shape.has_arm(a) && shape.has_arm(b));
            assert!(!shape.has_arm(a.opposite()));
            assert_eq!(shape.bend_name(), name);
            assert_eq!(Shape::from_bend(shape.axis(), name), Some(shape));
        }
        assert_eq!(Shape::corner(Dir::North, Dir::North), None);
        assert_eq!(Shape::corner(Dir::Up, Dir::Down), None);
        assert_eq!(Shape::corner(Dir::East, Dir::West), None);
        assert_eq!(Shape::straight(Axis::X).bend_name(), "none");
        assert_eq!(
            Shape::from_bend(Axis::Z, "none"),
            Some(Shape::straight(Axis::Z))
        );
        assert_eq!(Shape::from_bend(Axis::X, "north_south"), None);
        assert_eq!(Shape::from_bend(Axis::X, "sideways"), None);
    }

    #[test]
    fn a_tee_names_its_three_arms_in_canonical_order() {
        for (name, a, b, c) in TEE_NAMES {
            let shape = Shape::tee(a, b, c).expect("three distinct arms are a T");
            let mut wanted = [a, b, c];
            wanted.sort_by_key(|dir| dir.index());
            assert_eq!(shape.arms(), wanted, "{name} keeps canonical arm order");
            assert!(shape.has_arm(a) && shape.has_arm(b) && shape.has_arm(c));
            for missing in Dir::ALL {
                if !wanted.contains(&missing) {
                    assert!(!shape.has_arm(missing), "{name} has no {}", missing.name());
                }
            }
            assert_eq!(shape.bend_name(), name);
            assert_eq!(Shape::from_bend(shape.axis(), name), Some(shape));
            // A triple is not a corner: two names never alias.
            assert!(corner_from_name(name).is_none(), "{name}");
        }
        assert_eq!(Shape::tee(Dir::North, Dir::East, Dir::North), None);
        assert_eq!(Shape::tee(Dir::Up, Dir::Up, Dir::Down), None);
        assert!(Shape::from_bend(Axis::X, "up_north_east").is_some());
        assert_eq!(Shape::from_bend(Axis::X, "up_north_east_sideways"), None);
        assert_eq!(Shape::from_bend(Axis::X, "up_down"), None);
    }

    #[test]
    fn the_state_model_stays_bounded() {
        // 1 none + 12 corner pairs + 20 arm triples = 33 bend values;
        // 3 axis (the template property) x 33 x 16 power = 1584 states.
        assert_eq!(CORNER_NAMES.len(), 12);
        assert_eq!(TEE_NAMES.len(), 20);
        let bend_values = 1 + CORNER_NAMES.len() + TEE_NAMES.len();
        let states = 3 * bend_values * 16;
        assert_eq!(states, 1584);
        assert!(states < 2000, "the model stays under the state budget");
    }

    #[test]
    fn a_tee_answers_only_on_its_three_arms() {
        let tee = Shape::tee(Dir::Down, Dir::North, Dir::East).expect("a T");
        for arm in tee.arms() {
            assert_eq!(track_signal(9, tee, *arm), 9, "{}", arm.name());
        }
        for missing in [Dir::Up, Dir::South, Dir::West] {
            assert_eq!(track_signal(9, tee, missing), 0, "{}", missing.name());
        }
        assert_eq!(track_signal(20, tee, Dir::East), 15);
    }

    #[test]
    fn a_tee_connects_only_on_an_arm_a_neighbour_points_back_at() {
        let here = [0, 64, 0];
        let tee = Shape::tee(Dir::West, Dir::East, Dir::South).expect("a T");
        // All three arms join the matching straight track.
        assert!(connected(tee, here, Shape::straight(Axis::X), [1, 64, 0]));
        assert!(connected(tee, here, Shape::straight(Axis::X), [-1, 64, 0]));
        assert!(connected(tee, here, Shape::straight(Axis::Z), [0, 64, 1]));
        // The fourth side is not an arm; a corner that does not point back fails.
        assert!(!connected(tee, here, Shape::straight(Axis::Z), [0, 64, -1]));
        assert!(!connected(tee, here, Shape::straight(Axis::Y), [0, 65, 0]));
        let corner = Shape::corner(Dir::North, Dir::West).expect("corner");
        assert!(!connected(tee, here, corner, [-1, 64, 0]));
        // Two tees join arm to arm.
        let other = Shape::tee(Dir::East, Dir::North, Dir::Up).expect("a T");
        assert!(connected(tee, here, other, [-1, 64, 0]));
    }

    #[test]
    fn a_shape_keeps_the_axis_its_state_writes() {
        assert_eq!(Shape::straight(Axis::Y).axis(), Axis::Y);
        // A corner's first arm in canonical order names the state's axis.
        assert_eq!(
            Shape::corner(Dir::North, Dir::East).expect("corner").axis(),
            Axis::Z
        );
        assert_eq!(
            Shape::corner(Dir::Down, Dir::East).expect("corner").axis(),
            Axis::Y
        );
        assert_eq!(
            Shape::corner(Dir::West, Dir::South).expect("corner").axis(),
            Axis::Z
        );
    }

    #[test]
    fn a_corner_answers_only_on_its_two_arms() {
        let corner = Shape::corner(Dir::East, Dir::Down).expect("corner");
        assert_eq!(corner.arms(), [Dir::Down, Dir::East]);
        assert_eq!(track_signal(9, corner, Dir::Down), 9);
        assert_eq!(track_signal(9, corner, Dir::East), 9);
        assert_eq!(track_signal(9, corner, Dir::West), 0);
        assert_eq!(track_signal(9, corner, Dir::Up), 0);
        assert_eq!(track_signal(9, corner, Dir::North), 0);
    }

    #[test]
    fn a_corner_carries_the_signal_around_the_turn_with_one_step() {
        let corner = Shape::corner(Dir::West, Dir::North).expect("corner");
        assert_eq!(corner.arms(), [Dir::North, Dir::West]);
        // In on the west arm from a track at 15, out the north arm at 14.
        let power = track_power(&[Supply::None, Supply::Track(15)]);
        assert_eq!(power, 14);
        assert_eq!(track_signal(power, corner, Dir::North), 14);
        assert_eq!(track_signal(power, corner, Dir::West), 14);
        // The next straight track around the corner decays once more.
        assert_eq!(supply_power(Supply::Track(power)), 13);
        // A source on the north arm wins over the decayed track.
        assert_eq!(track_power(&[Supply::Source(15), Supply::Track(7)]), 15);
        // A track that is not on an arm supplies nothing.
        assert_eq!(supply_power(Supply::None), 0);
    }

    #[test]
    fn a_corner_connects_only_where_both_tracks_hold_the_arm() {
        let here = [0, 64, 0];
        let corner = Shape::corner(Dir::North, Dir::West).expect("corner");
        let x = Shape::straight(Axis::X);
        let z = Shape::straight(Axis::Z);
        // The corner's north arm joins a z track that reaches south.
        assert!(connected(corner, here, z, [0, 64, -1]));
        // ... but not an x track there, which has no south arm.
        assert!(!connected(corner, here, x, [0, 64, -1]));
        // Its west arm joins an x track to the west.
        assert!(connected(corner, here, x, [-1, 64, 0]));
        // Nothing on its back (south) or behind its arms (east).
        assert!(!connected(corner, here, z, [0, 64, 1]));
        assert!(!connected(corner, here, x, [1, 64, 0]));
        // A straight track does not reach a corner it has no arm for.
        assert!(!connected(x, here, corner, [0, 64, -1]));
        // Two corners join arm to arm.
        let south_east = Shape::corner(Dir::South, Dir::East).expect("corner");
        assert!(connected(corner, here, south_east, [0, 64, -1]));
    }

    #[test]
    fn a_side_click_bends_a_connected_straight_track() {
        let x = Shape::straight(Axis::X);
        // Wired from the west: a click on the north side keeps west and turns.
        assert_eq!(
            click_action(x, Dir::North, &[true, false], ClickMode::Draw),
            ClickAction::Bend(Shape::corner(Dir::West, Dir::North).expect("corner"))
        );
        // Wired from the east: the same side click keeps east.
        assert_eq!(
            click_action(x, Dir::North, &[false, true], ClickMode::Draw),
            ClickAction::Bend(Shape::corner(Dir::East, Dir::North).expect("corner"))
        );
        // A corner with its west arm wired turns its unwired north arm south.
        let corner = Shape::corner(Dir::West, Dir::North).expect("corner");
        assert_eq!(
            click_action(corner, Dir::South, &[false, true], ClickMode::Draw),
            ClickAction::Bend(Shape::corner(Dir::South, Dir::West).expect("corner"))
        );
    }

    #[test]
    fn a_side_click_with_two_connected_arms_widens_the_net_to_a_tee() {
        // Both arms of a straight track reach a neighbour: the side click adds
        // the third arm instead of being refused (upstream's net can hold it).
        let x = Shape::straight(Axis::X);
        assert_eq!(
            click_action(x, Dir::Up, &[true, true], ClickMode::Draw),
            ClickAction::Widen(Shape::tee(Dir::West, Dir::East, Dir::Up).expect("a T"))
        );
        // A corner with both arms wired widens the same way.
        let corner = Shape::corner(Dir::North, Dir::West).expect("corner");
        assert_eq!(
            click_action(corner, Dir::Down, &[true, true], ClickMode::Draw),
            ClickAction::Widen(Shape::tee(Dir::Down, Dir::North, Dir::West).expect("a T"))
        );
        // A third arm cannot grow a fourth one; an arm click still extends.
        let tee = Shape::tee(Dir::West, Dir::East, Dir::South).expect("a T");
        assert_eq!(
            click_action(tee, Dir::North, &[true, true, true], ClickMode::Draw),
            ClickAction::Refuse
        );
        assert_eq!(
            click_action(tee, Dir::North, &[false, false, false], ClickMode::Draw),
            ClickAction::Erase
        );
        assert_eq!(
            click_action(tee, Dir::East, &[true, false, false], ClickMode::Draw),
            ClickAction::Extend(Dir::East)
        );
    }

    #[test]
    fn a_sneak_click_erases_a_wired_track_without_cutting_its_neighbours() {
        // A wired two-arm track is erased by the sneak click (slice 3 required
        // cutting its neighbours first).
        let x = Shape::straight(Axis::X);
        assert_eq!(
            click_action(x, Dir::East, &[true, true], ClickMode::Erase),
            ClickAction::Erase
        );
        assert_eq!(
            click_action(x, Dir::Up, &[true, false], ClickMode::Erase),
            ClickAction::Erase
        );
        // A T drops only the clicked arm and keeps the other two.
        let tee = Shape::tee(Dir::West, Dir::East, Dir::South).expect("a T");
        assert_eq!(
            click_action(tee, Dir::South, &[true, true, true], ClickMode::Erase),
            ClickAction::Narrow(Shape::straight(Axis::X))
        );
        let tee = Shape::tee(Dir::West, Dir::North, Dir::South).expect("a T");
        assert_eq!(
            click_action(tee, Dir::South, &[true, false, true], ClickMode::Erase),
            ClickAction::Narrow(Shape::corner(Dir::North, Dir::West).expect("corner"))
        );
        // A side click (not an arm) erases the whole cell.
        assert_eq!(
            click_action(tee, Dir::East, &[true, true, true], ClickMode::Erase),
            ClickAction::Erase
        );
    }

    #[test]
    fn a_bend_towards_the_opposite_arm_is_refused() {
        // The north arm is wired; the south side is its back, so a bend would
        // drop the corner and straighten the line across the block.
        let corner = Shape::corner(Dir::West, Dir::North).expect("corner");
        assert_eq!(
            click_action(corner, Dir::South, &[true, false], ClickMode::Draw),
            ClickAction::Refuse
        );
        // An unwired opposite side is a plain erase, not a bend.
        assert_eq!(
            click_action(corner, Dir::South, &[false, false], ClickMode::Draw),
            ClickAction::Erase
        );
        // Clicking an arm always extends, wired or not.
        assert_eq!(
            click_action(corner, Dir::West, &[false, false], ClickMode::Draw),
            ClickAction::Extend(Dir::West)
        );
    }

    #[test]
    fn a_relay_outputs_away_from_the_block_it_is_placed_on() {
        assert_eq!(relay_facing(Dir::North), Dir::South);
        assert_eq!(relay_facing(Dir::South), Dir::North);
        assert_eq!(relay_facing(Dir::Up), Dir::Down);
        assert_eq!(relay_facing(Dir::East), Dir::West);
    }

    #[test]
    fn relay_variants_round_trip_their_block_ids() {
        for kind in RelayKind::ALL {
            assert_eq!(RelayKind::from_block_id(kind.block_id()), Some(kind));
        }
        assert_eq!(RelayKind::from_block_id("redstonepen:bridge_relay"), None);
        assert!(!RelayKind::Plain.has_state());
        assert!(!RelayKind::Inverted.has_state());
        assert!(RelayKind::Pulse.has_state());
        assert!(RelayKind::Bistable.has_state());
    }

    #[test]
    fn the_plain_relay_outputs_its_input_and_only_from_the_output_face() {
        let on = RelayPhase {
            powered: true,
            state: false,
        };
        let off = RelayPhase::default();
        assert!(relay_output(RelayKind::Plain, on));
        assert!(!relay_output(RelayKind::Plain, off));
        assert_eq!(
            relay_variant_signal(RelayKind::Plain, on, Dir::East, Dir::East),
            15
        );
        assert_eq!(
            relay_variant_signal(RelayKind::Plain, on, Dir::East, Dir::West),
            0
        );
        assert_eq!(
            relay_variant_signal(RelayKind::Plain, off, Dir::East, Dir::East),
            0
        );
    }

    #[test]
    fn the_inverted_relay_outputs_the_negation_of_its_input() {
        // Truth table: an unpowered inverted relay is lit; a powered one dark.
        let off = RelayPhase::default();
        let on = RelayPhase {
            powered: true,
            state: false,
        };
        assert!(relay_output(RelayKind::Inverted, off));
        assert!(!relay_output(RelayKind::Inverted, on));
        assert_eq!(
            relay_variant_signal(RelayKind::Inverted, off, Dir::Up, Dir::Up),
            15
        );
        assert_eq!(
            relay_variant_signal(RelayKind::Inverted, off, Dir::Up, Dir::Down),
            0
        );
        assert_eq!(
            relay_variant_signal(RelayKind::Inverted, on, Dir::Up, Dir::Up),
            0
        );
    }

    #[test]
    fn the_bistable_relay_toggles_on_a_rising_input_edge_and_holds() {
        let idle = RelayPhase::default();
        let armed = relay_edge(RelayKind::Bistable, true, idle).expect("rising edge toggles");
        assert!(armed.powered && armed.state);
        let held = relay_edge(RelayKind::Bistable, false, armed).expect("falling clears POWERED");
        assert!(!held.powered && held.state);
        let toggled_off =
            relay_edge(RelayKind::Bistable, true, held).expect("second rising toggles");
        assert!(toggled_off.powered && !toggled_off.state);
        assert_eq!(relay_edge(RelayKind::Bistable, true, toggled_off), None);
        assert!(relay_output(RelayKind::Bistable, armed));
        assert!(!relay_output(RelayKind::Bistable, toggled_off));
    }

    #[test]
    fn the_pulse_relay_arms_a_two_tick_pulse_on_a_rising_edge() {
        assert_eq!(PULSE_TICKS, 2);
        let idle = RelayPhase::default();
        let armed = relay_edge(RelayKind::Pulse, true, idle).expect("rising edge arms");
        assert!(armed.powered && armed.state);
        assert!(relay_output(RelayKind::Pulse, armed));
        // A falling input does not cancel the latch; the scheduled tick does.
        let falling = relay_edge(RelayKind::Pulse, false, armed).expect("falling clears POWERED");
        assert!(!falling.powered && falling.state);
        // The pulse clears to STATE=0 after PULSE_TICKS.
        let cleared = RelayPhase {
            powered: true,
            state: false,
        };
        assert!(!relay_output(RelayKind::Pulse, cleared));
        assert_eq!(relay_edge(RelayKind::Pulse, true, cleared), None);
    }

    #[test]
    fn plain_and_inverted_relays_delay_only_turning_their_output_off() {
        assert_eq!(RELAY_OFF_DELAY_TICKS, 2);
        let on = RelayPhase {
            powered: true,
            state: false,
        };
        let off = RelayPhase::default();
        // Plain: a falling input turns the output off two ticks later.
        assert!(relay_delayed_off(RelayKind::Plain, false, on));
        assert!(!relay_delayed_off(RelayKind::Plain, true, off));
        // Inverted: a rising input turns its inverted output off two ticks later.
        assert!(relay_delayed_off(RelayKind::Inverted, true, off));
        assert!(!relay_delayed_off(RelayKind::Inverted, false, on));
        assert!(!relay_delayed_off(RelayKind::Bistable, true, off));
        assert!(!relay_delayed_off(RelayKind::Pulse, true, off));
    }

    #[test]
    fn a_variant_notifies_the_output_only_when_that_output_changes() {
        let idle = RelayPhase::default();
        let armed = RelayPhase {
            powered: true,
            state: false,
        };
        let latched = RelayPhase {
            powered: true,
            state: true,
        };
        assert!(relay_notifies(RelayKind::Plain, true, idle));
        assert!(relay_notifies(RelayKind::Inverted, false, armed));
        assert!(relay_notifies(RelayKind::Bistable, true, idle));
        assert!(!relay_notifies(RelayKind::Bistable, false, latched));
        assert!(relay_notifies(RelayKind::Pulse, true, idle));
        // A second rising edge while the pulse is still high does not re-notify.
        assert!(!relay_notifies(RelayKind::Pulse, true, latched));
        assert!(!relay_notifies(RelayKind::Pulse, false, latched));
    }
}
