// A player joined the host's world: the relay sends to them.
fn ayynoxs_player_in(player: u128) {
    crate::game::ayynoxs_map::player_joined(player);
}
