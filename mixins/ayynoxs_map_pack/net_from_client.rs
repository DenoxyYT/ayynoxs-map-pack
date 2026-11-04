// A guest's mod message, on the host: welcome them or pass it on.
fn ayynoxs_net_from_client(game: &mut crate::game::Game<'_>, player: u128, channel: &str, data: &[u8]) {
    crate::game::ayynoxs_map::net_from_client(game, player, channel, data);
}
