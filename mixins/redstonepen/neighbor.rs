// Neighbour change: queue the track or relay for the next level tick.
fn observe_neighbor_changed(
    level: &crate::game::GameLevel<'_>,
    dimension: LevelDimension,
    pos: BlockPos,
    state: BlockStateId,
) {
    crate::game::redstonepen_state::on_neighbor_changed(level, dimension, pos, state);
}
