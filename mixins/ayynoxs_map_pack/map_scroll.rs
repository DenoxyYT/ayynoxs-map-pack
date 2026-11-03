// The wheel over the world map zooms around the pointer.
fn ayynoxs_map_scroll(game: &mut crate::game::Game<'_>, y: f32) -> std::ops::ControlFlow<()> {
    crate::game::ayynoxs_map::scroll(game, y)
}
