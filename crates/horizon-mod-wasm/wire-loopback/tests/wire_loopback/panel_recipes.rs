use super::*;
use mod_api::prelude::RecipesClientHost;
use mod_api::{PanelStyle, RecipeLayout, RecipeResult, RecipeTransferCheck, RecipeView, Stack};

thread_local! {
    static SHOWN: std::cell::RefCell<Option<(String, String, u32, Vec<RecipeView>)>> =
        const { std::cell::RefCell::new(None) };
}

struct Client;
impl mod_api::Host for Client {}
mod_api::impl_host_areas!(Client);
impl mod_api::ClientHost for Client {}
mod_api::impl_client_host_areas!(Client, except(recipes));
// A boundary reply fixture: the rest of the area is not called.
impl RecipesClientHost for Client {
    fn panel_items(&mut self, _: &str, _: Vec<Stack>) {
        unreachable!()
    }
    fn panel_style(&mut self, _: &str, _: PanelStyle) {
        unreachable!()
    }
    fn panel_recipes(
        &mut self,
        mod_id: &str,
        title: &str,
        tabs: Vec<Stack>,
        tab: u32,
        recipes: Vec<RecipeView>,
    ) {
        assert!(tabs.is_empty());
        SHOWN.set(Some((mod_id.into(), title.into(), tab, recipes)));
    }
    fn panel_owner(&self) -> Option<String> {
        Some("viewer".into())
    }
    fn place_recipe(&mut self, _: i32, _: bool) -> Result<(), String> {
        unreachable!()
    }
    fn transfer_recipe(&mut self, _: i32) {
        unreachable!()
    }
    fn transfer_recipe_id(&mut self, _: &str, _: bool) {
        unreachable!()
    }
    fn transfer_recipe_check(&mut self, _: i32) -> Option<RecipeTransferCheck> {
        unreachable!()
    }
    fn recipe_book_ids(&mut self, _: &str) -> Arc<[i32]> {
        unreachable!()
    }
}

#[test]
fn a_recipe_page_and_the_panel_owner_cross_the_host_call() {
    install_counter();
    let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
    let mut client = Client;
    let mut frame = boundary::Frame::default();
    frame.set_client_host_mut(&mut client);
    let mut cx = boundary::Cx::default();
    cx.frames.push(frame);
    let cx = std::cell::RefCell::new(cx);
    guest::set_transport(Some(Rc::new(move |id, bytes| {
        let mut out = Writer::new();
        // SAFETY: the client and its frame outlive this synchronous transport.
        unsafe { boundary::serve(id, &mut *cx.borrow_mut(), bytes, &mut out) }.unwrap();
        out.into_bytes()
    })));
    let recipes = vec![
        RecipeView {
            width: 2,
            height: 3,
            slots: vec![Vec::new(); 4],
            results: vec![
                RecipeResult::default(),
                RecipeResult {
                    stacks: Vec::new(),
                    chance: Some(12.5),
                },
            ],
            note: "200 ticks".into(),
            action: Some(false),
            ..RecipeView::default()
        },
        RecipeView {
            layout: RecipeLayout::Brewing,
            action: Some(true),
            ..RecipeView::default()
        },
    ];
    guest::Guest.panel_recipes("crusher", "Crushing", Vec::new(), 3, recipes.clone());
    assert_eq!(
        SHOWN.take(),
        Some(("crusher".into(), "Crushing".into(), 3, recipes))
    );
    assert_eq!(guest::Guest.panel_owner().as_deref(), Some("viewer"));
    guest::set_transport(None);
}
