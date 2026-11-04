// A mod message from the server: what other players shared.
fn ayynoxs_net_from_server(game: &mut crate::game::Game<'_>, channel: &str, data: &[u8]) {
    crate::game::ayynoxs_map::net_from_server(game, channel, data);
}
