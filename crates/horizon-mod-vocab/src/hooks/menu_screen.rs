//! Layout for a content menu that has no matching block-owned screen.

pub trait Hooks {
    /// Lay out a content menu opened without a matching mod block, keyed by its
    /// registered menu type. Place its container slots by index; a slot left
    /// `None`, or past the list, is hidden. The server asks it too, when
    /// `PlayerRef::open_mod_menu` opens the menu: no click on its backed slots
    /// (`back_mod_menu_slots`) moves an item into or out of a slot it hides, so
    /// shift-clicks never fill it, as for a block's `screen`. While it answers,
    /// `screen_extra_data` and `screen_pos` read the opener's `extra` and `pos`,
    /// on both sides, so the layout can follow them. Leaving the answer empty
    /// closes the window. The block-scoped screen and draw hooks still handle
    /// block-owned menus. To add controls, use the screen render and input
    /// hooks, matching the menu type in `ScreenView::menu`.
    #[hook(
        side = "client",
        contract = "replace",
        out = "screen",
        scope = "menu",
        key = "menu",
        neoforge = "RegisterMenuScreensEvent"
    )]
    fn menu_screen(menu: &str, screen: &mut Option<Screen>);
}
