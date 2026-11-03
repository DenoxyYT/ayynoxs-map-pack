// The minimap Horizon draws: the pack's corner, size, zoom and shape.
fn ayynoxs_minimap(
    game: &crate::game::Game<'_>,
    minimap: &mut Option<crate::game::native_hooks::NativeMinimap>,
) {
    crate::game::ayynoxs_map::minimap(game, minimap);
}
