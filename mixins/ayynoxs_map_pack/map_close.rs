// Esc on the world map closes it instead of the Advancements screen.
fn ayynoxs_map_close(game: &mut crate::game::Game<'_>) -> std::ops::ControlFlow<()> {
    crate::game::ayynoxs_map::close(game)
}
