// Coordinates and direction under the minimap, and the waypoints on it.
fn ayynoxs_hud(
    game: &crate::game::Game<'_>,
    batch: &mut horizon_client::hud::HudBatch,
    corners: &mut horizon_client::extras_hud::Corners,
) {
    crate::game::ayynoxs_map::hud(game, batch, corners);
}
