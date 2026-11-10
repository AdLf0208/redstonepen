// End of the level tick: apply the queued relays and tracks.
fn observe_level_tick_post(game: &mut crate::game::Game<'_>, dimension: &str) {
    crate::game::redstonepen_state::on_level_tick_post(game, dimension);
}
