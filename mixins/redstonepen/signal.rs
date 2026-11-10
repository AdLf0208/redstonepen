// `block_signal`: answer from the track's stored power or the relay's state
// (hot path; no writes).
fn observe_block_signal(
    level: &crate::game::GameLevel<'_>,
    pos: BlockPos,
    state: BlockStateId,
    face: Direction,
    direct: bool,
    signal: &mut i32,
) {
    crate::game::redstonepen_state::on_block_signal(level, pos, state, face, direct, signal);
}
