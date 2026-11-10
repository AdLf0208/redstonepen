// `getStateForPlacement`: the track's axis or the relay's output face.
fn observe_state_for_placement(
    level: &crate::game::GameLevel<'_>,
    state: BlockStateId,
    context: &PlacementContext,
    answer: &mut Option<Option<BlockStateId>>,
) {
    crate::game::redstonepen_state::on_state_for_placement(level, state, context, answer);
}
