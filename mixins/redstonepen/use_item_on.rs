// A pen click on a support draws a track, on a track extends or erases it.
fn observe_use_item_on(
    game: &mut crate::game::Game<'_>,
    stack: &horizon_client::items::ItemStack,
    pos: [i32; 3],
    face: [i32; 3],
    claimed: &mut bool,
) {
    crate::game::redstonepen_state::on_use_item_on(game, stack, pos, face, claimed);
}
