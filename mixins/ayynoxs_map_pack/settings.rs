// The settings page of the minimap or the world map.
fn ayynoxs_page(game: &mut crate::game::Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    crate::game::ayynoxs_map::settings_page(game, mod_id, buttons);
}
