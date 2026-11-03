// The world map, drawn in the Advancements screen's place while it is open.
fn ayynoxs_map_draw(
    game: &mut crate::game::Game<'_>,
    batch: &mut horizon_client::hud::HudBatch,
) -> std::ops::ControlFlow<()> {
    crate::game::ayynoxs_map::draw(game, batch)
}
