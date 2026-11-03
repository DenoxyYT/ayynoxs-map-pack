// The left button on the world map: start or end a drag.
fn ayynoxs_map_mouse(game: &mut crate::game::Game<'_>, pressed: bool) -> std::ops::ControlFlow<()> {
    crate::game::ayynoxs_map::mouse(game, pressed)
}
