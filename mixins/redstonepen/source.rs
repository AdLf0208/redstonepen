// `is_signal_source`: a track and a relay both answer the signal query.
fn observe_is_signal_source(
    level: &crate::game::GameLevel<'_>,
    state: BlockStateId,
    source: &mut bool,
) {
    crate::game::redstonepen_state::on_is_signal_source(level, state, source);
}
