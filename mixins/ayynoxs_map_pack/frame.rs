// Once a frame: explore the loaded chunks, note deaths, save.
fn ayynoxs_frame(game: &mut crate::game::Game<'_>) {
    crate::game::ayynoxs_map::frame(game);
}
