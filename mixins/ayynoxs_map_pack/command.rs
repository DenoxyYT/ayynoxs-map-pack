// `/wp add|del|list|clear`.
fn ayynoxs_command(game: &mut crate::game::Game<'_>, command: &str, ran: &mut bool) {
    crate::game::ayynoxs_map::command(game, command, ran);
}
