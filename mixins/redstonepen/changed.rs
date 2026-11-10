// A placed/changed block: seed the first evaluation and the neighbours.
fn observe_block_changed(
    level: &crate::game::GameLevel<'_>,
    dimension: LevelDimension,
    pos: BlockPos,
    new: BlockStateId,
) {
    crate::game::redstonepen_state::on_block_changed(level, dimension, pos, new);
}
