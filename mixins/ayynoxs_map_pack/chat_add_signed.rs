// A player's chat line (a /msg): a shared waypoint's message stays out of the chat.
fn ayynoxs_chat_add_signed(content: &crate::text::Text) -> std::ops::ControlFlow<()> {
    if crate::ayynoxs_chat::seen(content) {
        return std::ops::ControlFlow::Break(());
    }
    std::ops::ControlFlow::Continue(())
}
