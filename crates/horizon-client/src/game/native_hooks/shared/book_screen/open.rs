use super::*;

impl Game<'_> {
    /// The container slots (of `size`) that the mod's screen for the block at
    /// `pos` does not show: the server's menu does not have them either, so a
    /// shift-click never moves an item where the player cannot see it.
    pub(in crate::game) fn native_hidden_slots(&self, pos: Pos, size: usize) -> Vec<usize> {
        let Some(menu) = self.mod_menu_at(pos) else {
            return Vec::new();
        };
        let block = self
            .res
            .facts()
            .get(self.world.block(pos.0, pos.1, pos.2))
            .name
            .to_string();
        let mut screen = mod_api::RecipeBookLayout::registered_screen(menu);
        if screen.is_none() {
            self.native_screen(&block, &mut screen);
        }
        let Some(screen) = screen else {
            return Vec::new();
        };
        (0..size)
            .filter(|&i| screen.slots.get(i).is_none_or(Option::is_none))
            .collect()
    }
    /// Resolve a mod block only if its container opens this exact menu type.
    /// A clicked target may be left over from a previous interaction.
    fn screen_block_for_menu(&self, target: Pos, menu: &str) -> Option<String> {
        let block = &self
            .res
            .facts()
            .get(self.world.block(target.0, target.1, target.2))
            .name;
        if block.starts_with("minecraft:") {
            return None;
        }
        let mut container = None;
        self.native_block_entity_container(block, &mut container);
        let opens = container.and_then(|c| mod_menu(c.menu.as_deref()?).map(|(id, _)| id));
        (opens == Some(menu)).then(|| block.to_string())
    }

    /// The layout `menu_screen` gives content menu `menu` opened with no
    /// matching block container, from `extra` and `pos` (what the opener gave
    /// `player_open_mod_menu`, which the part reads with `screen_extra_data`
    /// and `screen_pos` while it answers). A slot it leaves `None`, or past its
    /// list, is hidden; every slot it places needs finite coordinates. Asked on
    /// the client to show the screen and on the server to hide those slots.
    /// The recipe-book layout's server placement is scoped to a block container.
    pub(in crate::game) fn blockless_menu_screen(
        &self,
        menu: &str,
        extra: &[u8],
        pos: Option<[i32; 3]>,
    ) -> Option<mod_api::Screen> {
        let menu_type = horizon_client::containers::mod_menu_type(menu)?;
        let mut screen = None;
        {
            let _opening =
                crate::game::native_hooks::menu_extra_data::MenuScreenOpening::enter(extra, pos);
            self.native_menu_screen(mod_api::__private::MenuScreenCall {
                menu,
                screen: &mut screen,
            });
        }
        let screen = screen?;
        if screen.slots.len() > menu_type.slots
            || screen
                .slots
                .iter()
                .flatten()
                .any(|pos| pos.iter().any(|coord| !coord.is_finite()))
            || ![screen.width, screen.height]
                .into_iter()
                .all(|size| size.is_finite() && size > 0.0)
            || ![screen.inventory_x, screen.inventory_y]
                .into_iter()
                .all(f32::is_finite)
        {
            log::warn!(
                "Horizon: menu_screen for {menu} must have finite geometry and at most {} container slots",
                menu_type.slots
            );
            return None;
        }
        Some(screen)
    }

    /// The container slots (of `size`) that `menu_screen` hides for content
    /// menu `menu` opened with no block: on the server, no click on the menu's
    /// backed slots reaches them, as [`Self::native_hidden_slots`] keeps a
    /// block's screen's hidden slots out of its menu.
    pub(in crate::game) fn blockless_hidden_slots(
        &self,
        menu: &str,
        extra: &[u8],
        pos: Option<[i32; 3]>,
        size: usize,
    ) -> Vec<usize> {
        let Some(screen) = self.blockless_menu_screen(menu, extra, pos) else {
            return Vec::new();
        };
        (0..size)
            .filter(|&i| screen.slots.get(i).is_none_or(Option::is_none))
            .collect()
    }

    /// Open the matching block's screen or the content menu type's layout.
    /// `extra` is what the server sent for this window first.
    pub(in crate::game) fn open_native_screen(
        &mut self,
        key: Pos,
        target: Option<Pos>,
        window: i32,
        menu: &str,
        extra: Option<horizon_protocol::menu_extra::Message>,
    ) {
        self.clear_mod_book_placement_screen();
        self.mod_screen = None;
        let block = target.and_then(|pos| self.screen_block_for_menu(pos, menu));
        let screen = match &block {
            Some(block) => {
                let mut screen = mod_api::RecipeBookLayout::registered_screen(menu);
                if screen.is_none() {
                    self.native_screen(block, &mut screen);
                }
                screen
            }
            None => {
                let data = extra.as_ref().map_or(&[][..], |e| e.data.as_slice());
                self.blockless_menu_screen(menu, data, extra.as_ref().and_then(|e| e.pos))
            }
        };
        if let Some(screen) = screen {
            self.mod_screen = Some(OpenNativeScreen {
                layout_revision: {
                    static NEXT: std::sync::atomic::AtomicU64 =
                        std::sync::atomic::AtomicU64::new(1);
                    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                },
                key,
                window,
                block,
                screen,
                data: Vec::new(),
                extra,
            });
            self.begin_mod_book_placement();
            self.ensure_mod_book_catalog();
        }
    }
}
