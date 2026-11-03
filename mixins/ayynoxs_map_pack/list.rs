// The pack's two rows in Options > Mods, in the chosen language.
fn ayynoxs_listed(
    game: &crate::game::Game<'_>,
    mods: &mut Vec<crate::game::native_hooks::NativeMod>,
) {
    crate::game::ayynoxs_map::listed(game, mods);
}
