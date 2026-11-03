// The pointer moved over the world map: pan while dragging.
fn ayynoxs_map_drag(game: &mut crate::game::Game<'_>) -> std::ops::ControlFlow<()> {
    crate::game::ayynoxs_map::drag(game)
}
