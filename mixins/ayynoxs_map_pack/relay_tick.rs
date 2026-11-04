// Each server tick: the host's own shares go to its guests.
fn ayynoxs_relay_tick(game: &mut crate::game::Game<'_>) {
    crate::game::ayynoxs_map::relay_tick(game);
}
