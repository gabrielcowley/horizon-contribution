# Show recipe pages in the item panel

On the client, `panel_recipes(mod_id, title, tabs, tab, recipes)` shows recipes in the panel beside
container screens, in place of the mod's `panel_items` list. The game lays each recipe out with its
own slots, arrow and buttons, pages them to the panel's height, and draws `title`, a back button and
a row of `tabs` (each an icon stack; `tab` is the selected one) above them. It is a `WorldOnly`
operation, called through the client host.

```rust,ignore
let recipe = RecipeView {
    layout: RecipeLayout::Single,
    slots: vec![vec![gravel]],
    results: vec![
        RecipeResult { stacks: vec![flint.clone()], chance: None },
        RecipeResult { stacks: vec![flint], chance: Some(12.5) },
    ],
    fluid_inputs: vec![FluidStack::new(water, 250)],
    note: "200 ticks".into(),
    action: Some(true),
    ..RecipeView::default()
};
host.panel_recipes("mymod", "Crushing", vec![crusher_icon], 0, vec![recipe]);
```

`RecipeView` is built as a literal with `..RecipeView::default()`:

- `layout`: `Grid` (the default) draws a `width` by `height` grid, at most 3 by 3, whose extra
  slots continue in rows under it, 27 in all. `Cooking` draws one input over a flame, `Single` one
  input, `Smithing` template, base and addition, and `Brewing` a potion and an ingredient.
- `slots` and each `RecipeResult`'s `stacks` cycle through their stacks, one a second, at most 64.
- `results`: up to four after the arrow. A `chance` in percent (0 to 100) is written under its
  slot; `None` is every time.
- `fluid_inputs` and `fluid_outputs`: up to eight each, each in a well with its amount in
  millibuckets under it.
- `note`: one short line under the recipe.
- `action`: an action button beside the recipe, enabled (`Some(true)`) or greyed (`Some(false)`).

At most 256 recipes and 16 tabs are shown; anything past a limit is cut. Showing the same title and
tab again keeps the page the player turned to, so a mod can redraw its buttons without moving them.

The player's input comes back through the `panel_event` hook as `PanelEvent`: `SlotClicked` (the
recipe's index, the slot's, results after the inputs, and the stack it showed), `Action`, `Page`,
`Tab` and `Back`. The game does nothing more on a tab or the back button: the mod shows the next
page itself, or calls `panel_recipes` with no recipes to show its item list again.

## Sharing the panel

The panel is one mod's at a time: the last to show something with `panel_items` or `panel_recipes`.
`panel_owner()` names that mod, or `None` when no mod has it (or the one that had it is turned off).
A mod that offers its pages only as a fallback, when no recipe viewer is using the panel, checks
`panel_owner()` first and shows nothing while another mod holds it. A mod gives the panel up by
showing no items and no recipes.
