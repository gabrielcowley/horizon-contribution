# Block entities, entities, particles and textured faces in screen scenes

`DrawContext::draw_scene` draws a `Scene` in a screen, as a guide page or a Ponder scene
shows a structure. A scene without a clock just draws its blocks and keeps nothing. A
clocked scene can tick copies of mod block entities, pose mobs and animate particles. This is
GuideME's living scene: a lit furnace shows its flames and smoke, a campfire smokes, fire
flickers, and dripstone and leaves drip, the way the client's animate tick makes these
particles for blocks near the player.

```rust
use mod_api::{GuiEntity, Scene, SceneClock, SceneEntity, SceneView};

// Blocks alone: no clock, no state.
event.batch.draw_scene(rect, &Scene::new(blocks.clone()), view)?;

// Build a living scene once and keep it. Change only its clock from frame to frame.
let mut scene = Scene::new(blocks)
    .with_clock(SceneClock::new(1, 0, 0.0))
    .with_entities(vec![
        SceneEntity::new(GuiEntity::kind("minecraft:cow"), [1.5, 0.0, 0.5]).turned(135.0, 150.0, 0.0),
    ]);

// In `screen_draw`: the page's own tick count, 20 a second, and the frame's partial tick.
if let Some(clock) = &mut scene.clock {
    clock.tick = page_ticks;
    clock.partial = partial;
}
let view = SceneView::new([1.0, 0.5, 0.5], 225.0, 30.0, 18.0);
if let Err(why) = event.batch.draw_scene([12.0, 20.0, 140.0, 110.0], &scene, view) {
    log::warn!("scene not drawn whole: {why:?}");
}
```

## Pointing at a block

`SceneView::to_gui(rect, point)` gives the GUI position a scene point is drawn at in that box,
and `ray_from_gui(rect, gui)` the scene ray under a GUI point, for a label that points at a
block or for picking one under the mouse. They are the projection `draw_scene` uses, computed
in the mod with no host call. A section's pose is not applied: move the point by the section
first, or cast the ray through the section's inverse pose.

```rust
// The label's arrow points at the top of block [2, 1, 0].
let [x, y, _] = view.to_gui(rect, [2.5, 2.0, 0.5]);
// The ray under the mouse: march it through the scene's cells to find the block hit.
let (origin, direction) = view.ray_from_gui(rect, [mouse_x, mouse_y]);
```

`to_gui`'s third value is how far towards the viewer the point is, in GUI pixels; the ray's
origin is level with the view's center, and its direction points away from the viewer.

## Moving a section

Keep the blocks in their original cells. To lift or rotate a part of the structure, group those
blocks consecutively in `scene.blocks` and draw the scene with a `SceneSection` for that range:

```rust
use mod_api::SceneSection;

// Rotate blocks 2–5 about the hinge, then lift them half a block.
let door = SceneSection::new(2, 4, [0.0, 0.5, 0.0], [0.0, door_angle, 0.0], [2.0, 0.0, 1.0]);
event.batch.draw_scene_with_sections(rect, &scene, view, &[door])?;
```

The angles are degrees around scene X, Y, then Z; the pivot and the translation are in blocks.
List multiple sections in block-index order with no overlaps. Blocks outside the sections stay put.
A section's blocks and block-entity drawings move together, testing depth against the rest of the
scene. Faces at a moving boundary are kept so pulling a section away exposes both sides.

A pose changes drawing only. The scene clock, block animate ticks and particle collisions still use
the original cells; the scene's separate entity and particle positions do not follow the section.

## Entities

A `SceneEntity` holds the same `GuiEntity` that `DrawContext::entity` draws: a new entity
of a living type, with optional saved data (a baby, a sheep's colour), a dropped item
(`minecraft:item`, whose data needs its `Item`), a minecart of any kind, an entity of the
client's level as it is this frame, or a mod's general type built from data
(`GuiEntity::general` with a `GeneralRenderState`: its type, box and synced attachment
values). An item, a minecart or a general type is drawn by the route the world draws it
with: `ItemEntityRenderer`'s bob and spin, the minecart's body and display block, or the
renderer the general type registered (none draws nothing). A living one is drawn by its type's
renderer as in the world (a mod's `register_entity_renderers` renderer in place of the
vanilla model, then the `register_entity_layers` layers), full bright, shaded by face like
the scene's blocks and sorted with them by depth. Its feet stand at `pos` in the scene's
coordinates, turned by `yaw`, `head_yaw` and `head_pitch` in entity degrees (yaw 0 faces
south).

The entity is made for drawing only. It never joins a level and never ticks, and no mod
part's `register_goals` runs for it. Its age is the clock's tick, so the model's idle
animations and an item's bob follow the scene's clock. A scene takes at most 16 entities.

`SceneParts::entities` changes that for the entities it names, by index. A `SceneEntityPart`
with `in_group(g)` moves, turns and hides its entity with group `g` and lights it with the group's
light, as a minecart rides a moving section; its `pos` stays in the scene's coordinates before
the group's pose. `aged(t)` gives it an age of its own in place of the clock's tick.

## Mod block entities

A native mod can implement `BlockEntityPart::block_entity_scene_tick` on a Part attached
to its block entity **type** with `Registrar::block_entity_part`. This opts that type into
scene ticks, including when another mod displays it. The hook gets no host: it can change
the scene copy's NBT, item slots and menu data, but is never given a level or a live
block entity handle. The type's existing `block_entity_submit` renderer reads the changed
fields; it also sees the scene's tick and partial tick rather than the client's time.

```rust
use mod_api::{BlockEntityPart, BlockEntitySceneTickEvent, BlockPart};

struct Rotor;

#[mod_api::horizon_part]
impl BlockPart for Rotor {}

#[mod_api::horizon_part]
impl BlockEntityPart for Rotor {
    fn block_entity_scene_tick(&self, event: &mut BlockEntitySceneTickEvent<'_>) {
        if let Some(angle) = event.fields.data.first_mut() {
            *angle = (*angle + 5) % 360;
        }
    }
    // The existing block_entity_submit reads entity.data[0] as the angle.
}
```

The guide supplies the initial fields in a `SceneBlockEntity` and the block state it
belongs to. It doesn't register or call the other mod's ticker or renderer itself:

```rust
use mod_api::{Identifier, Scene, SceneBlock, SceneBlockEntity, SceneClock, item::containers::ModBlockEntityFields};

let mut fields = ModBlockEntityFields::default();
fields.data.push(0);
let rotor = SceneBlockEntity::new(Identifier::parse("machines:rotor")?, fields);
let scene = Scene::new(vec![SceneBlock::new([0, 0, 0], casing_state, Some(rotor))])
    .with_clock(SceneClock::new(2, page_ticks, partial).animate_blocks(false));
event.batch.draw_scene(rect, &scene, view)?;
```

`animate_blocks(false)` disables only block animate ticks (smoke, drips, etc.). The scene
copy starts at its supplied fields on the first draw and ticks as the scene clock advances.
You can rebuild the same inputs each frame without resetting the animation. Without a
registered scene ticker, the renderer uses the supplied fields. The live-world client and
server tick hooks never run for a scene.

### Time and `in_scene`

In a scene with a clock, every block entity is drawn on that clock, whether its type has a
scene ticker or not, and whether its mod is native or wasm: `draw.partial()` is the clock's
`partial`, `draw.game_time()` its `tick`, and `draw.animation_time()` is `tick + partial`,
which turns `spin` parts. Holding the tick holds them; moving it, forwards or back, moves
them. Without a clock, block entities are drawn on the client's clock.

`draw.in_scene()` is true for a block entity drawn in a scene and false in a level, as
Ponder's `isVirtual`. A renderer can skip what only makes sense in a level there.

## The scene's clock and its particles

Entities and particles need a clock. The scene keeps live particles and ticking block entity
copies between draws under the calling mod and `clock.id`: give each scene on your screens
its own id; another mod's id 1 is another scene.

- **A tick ahead** of the last tick the scene ran: the scene runs the ticks in between. On
  each tick the blocks run their animate ticks (if `animate_blocks` is set: 64 picks among
  the scene's own blocks), opted-in block entity copies run their scene tick, and particles
  move, collide with the scene's blocks and age. `Scene::particles` are added on the first
  tick run.
- **More than 20 ticks ahead** (a page resumed): the scene clears particles, restores the
  supplied block entity fields and runs only the last 20 ticks.
- **The same tick** again: the scene is paused. Nothing is added and nothing moves.
- **A tick behind**, or changed blocks, block entity inputs or entities: the scene starts
  over with no particles and the supplied block entity fields.

Each tick's randomness is seeded from the id and the tick, and the picks cover the scene's
blocks, not the view, so the same ticks always give the same picture from any angle. A
scene holds at most 512 live particles, the blocks' own and the mod's together; past that
no new one is added. The particles are drawn facing the viewer and cut off at the box. The
game's Particles option applies, as it does in the world. Scenes make no sound.

At most four living scenes animate in one frame. A fifth still draws its blocks and
its last state, but its clock does not advance, and the call returns `TooManyScenes`.

## Lines and solid quads

`DrawContext::draw_scene_with_overlays` draws annotations with the same camera and clip
rectangle as the scene. Positions are in scene blocks; line widths are in GUI pixels.
Colours are packed ARGB.

```rust
use mod_api::{SceneDepth, SceneOverlay};

let overlays = [
    SceneOverlay::line([0.0, 1.0, 0.0], [1.0, 1.0, 0.0], 2.0, 0xFFFF_FFFF, SceneDepth::Tested),
    SceneOverlay::quad(
        [[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]],
        0x8020_80FF,
        SceneDepth::ThroughWalls,
    ),
];
draw.draw_scene_with_overlays(rect, &scene, view, &overlays)?;
```

`Tested` hides an annotation behind nearer scene geometry. `ThroughWalls` ignores the
scene depth. Both draw after the scene, in list order, without writing depth; overlapping
annotations do not hide one another via depth testing. Invalid geometry or more than
`SCENE_MAX_OVERLAYS` entries refuses the whole draw with a `SceneOverlayError`. Ordinary
scene errors are carried as `SceneOverlayError::Scene`.

## Textured faces

Use `DrawContext::draw_scene_with_quads` to put a sprite on a scene-local face, for
example a fluid surface or a panel. It takes the same `Scene` and `SceneView` as
`draw_scene`, plus a slice of faces. Quads do not need or change the clock, and can
be used in an empty `Scene`. Both sides draw. The same scene box and depth buffer
apply to blocks, entities, particles and faces, so a nearer block can hide a quad.

```rust
use mod_api::{Scene, SceneTexturedQuad};

// A flat surface one block across, cropped to part of the flowing water sprite.
let surface = SceneTexturedQuad::new(
    "minecraft:block/water_flow",
    [[0.0, 0.8, 0.0], [1.0, 0.8, 0.0], [1.0, 0.8, 1.0], [0.0, 0.8, 1.0]],
    [[0.25, 0.25], [0.75, 0.25], [0.75, 0.75], [0.25, 0.75]],
    0xA03F_76E4,
);
draw.draw_scene_with_quads(rect, &Scene::new(blocks), view, &[surface])?;
```

List the four corners in order around the face, in blocks. Give each corner its own
normalized UV in the sprite (`[0, 0]` at its top left, `[1, 1]` at its bottom right).
The tint multiplies the sampled RGBA, including alpha; use `0xFFFF_FFFF` for no tint.
Use a sprite from the scene's existing GUI atlas, such as `block/water_still`,
`minecraft:block/water_overlay` or `mymod:gui/panel`. The atlas uses the player's
resource packs and updates animated sprites; no separate texture upload is needed.

`SceneWithQuadsError::BadTexturedQuad { index }` means a non-finite or out-of-range
corner (coordinates must be within 30 million blocks on each axis) or a UV outside
`0..=1`. `UnknownSprite { index }` means no sprite by that name is packed.
`TooManyTexturedQuads` means more than `SCENE_MAX_TEXTURED_QUADS` (4096) faces. These
errors draw nothing and leave the scene's state unchanged. Other scene refusals come
back wrapped in `SceneWithQuadsError::Scene`; for `TooManyScenes` the textured faces
draw along with the blocks and whatever last state is available, without advancing
the clock. The existing `draw_scene` call is unchanged.

## Sections, faces and overlays together

The calls above each draw a scene of their own, with a depth of its own. `draw_scene_parts` draws
sections, textured faces and overlays in one call, so a moved section hides a face behind it. It is
also where a scene chooses light, shading, a biome, fluids, the game's block entities and breaking
overlays. `SceneParts::new()` draws what `draw_scene` does; set only what the scene needs.

```rust
use mod_api::{Identifier, SceneFace, SceneGroup, SceneParts, SceneSection, SceneShading};

let rotor = SceneSection::new(4, 12, [0.0; 3], [0.0, angle, 0.0], [2.5, 0.0, 2.5]);
let parts = SceneParts::new()
    .with_groups(vec![SceneGroup::new(rotor).lit(9, 9)])
    .with_faces(vec![SceneFace::new(outline)])
    .with_shading(SceneShading::View { lights: [[0.4, -1.0, 0.7], [-0.4, -0.5, 0.7]] })
    .with_biome(Identifier::parse("minecraft:plains")?)
    .fluids(true)
    .with_game_block_entities(vec![(3, sign_data)])
    .with_breaking(vec![(7, stage)])
    .keep_particles(true);
draw.draw_scene_parts(rect, &scene, view, &parts)?;
```

- **Groups.** Each `SceneGroup` is a `SceneSection` with a light and a drawn flag, under the rules
  of "Moving a section". `lit(block, sky)` lights its blocks, fluids and block entities through the
  client's lightmap, as world blocks are; a mod block entity's renderer reads the same value from
  `light()`. A section fades by its light (Ponder's go from 5 to 15) and is never see-through.
  `drawn(false)` hides a section whose blocks still stand in the scene's level, so particles land
  on them.
- **Faces and overlays** follow the rules of "Textured faces" and "Lines and solid quads". They do
  not follow a section's pose. `SceneFace::tinted` gives each corner a tint of its own.
- **Shading.** `SceneShading::Faces` (the default) shades a face by its direction, as world blocks
  are. `SceneShading::View` lights blocks, fluids and block entities by two lights fixed to the
  view, as the game lights models in its GUI.
- **Biome.** Its colours tint grass, foliage and water. Without one, grass takes a fixed colour and
  water vanilla's default.
- **Fluids.** With `fluids(true)`, a block holding a fluid (or a waterlogged block) has its surface
  drawn as in the world. Heights, flow and hidden faces come from the blocks of its own section.
- **Game block entities.** A block with one of the game's block entity renderers (a chest, a sign,
  a banner, a bell) is drawn by it, with the saved data given for its index or else the type's
  defaults. The data's `id` may be left out. Nothing ticks, so chests stay shut; a player head
  draws its default skin. What moves with time, such as a banner's sway, follows the scene's
  clock when it has one, its tick as the game time and its partial tick, so it holds when the
  clock is paused and goes back when it is scrubbed back. Without a clock it follows the client's.
- **Breaking.** Destroy stage 0 to 9 over a block's model, by block index.
- **Keeping particles.** With `keep_particles(true)`, changed blocks, block entities or entities do
  not start the scene over: its particles, and the ticking copies whose inputs stay the same, go
  on. The scene also lives on through ticks that add no particle. A tick behind the last still
  starts over.

A bad part refuses the whole draw with a `ScenePartsError` and changes nothing kept:

| Error | Cause |
|---|---|
| `Scene(error)` | The scene itself, as the table below; `TooManyScenes` still draws |
| `BadSection { index }` | A group's block range or pose |
| `TooManyTexturedQuads`, `BadTexturedQuad { index }`, `UnknownSprite { index }` | A face |
| `TooManyOverlays`, `BadOverlay { index }` | An overlay |
| `NoBlock { index }` | Saved data or a breaking stage for a block the scene doesn't have |
| `BadBiome` | A biome the game doesn't have |
| `BadEntityPart { index }` | An entity part naming no entity, no group, or an entity another part names |

## Refusals

When `draw_scene` refuses a scene, it draws none of it (but for `TooManyScenes`) and
returns a `SceneError`:

| Error | Cause |
|---|---|
| `Unavailable` | Not a client's drawing |
| `BadView` | A view or box that isn't finite, or a scene too large to stay in finite GUI coordinates |
| `TooManyBlocks` | More than `SCENE_MAX_BLOCKS` (4096) blocks |
| `TooManyEntities` | More than `SCENE_MAX_ENTITIES` (16) entities |
| `TooManyParticles` | More than `SCENE_MAX_PARTICLES` (512) particles to add at once |
| `TooManyScenes` | Four other scenes animated this frame; the blocks and last state were drawn |
| `BadBlock { index }` | A block state the game doesn't have |
| `UnknownType { index }` | No entity type the client can draw by that id, as `DrawContext::entity` refuses it |
| `BadData { index }` | Entity data naming another type, or that doesn't load (an item without its `Item`) |
| `NoEntity { index }` | A handle naming no living entity of the client's level |
| `BadEntity { index }` | A position or turn that isn't finite, or a position more than 30 million blocks out |
| `BadParticle { index }` | Particle options, a position or a velocity that isn't finite, or options that name a stack |
| `BadClock` | A partial tick that isn't finite, with or without entities and particles |
| `NoClock` | Entities or particles without a clock |

## Not yet

- The breaking overlay is drawn with the GUI's ordinary blending, not the world's multiply.
- Additive (glowing) particle quads and pack (Polytone) particles are not drawn in scenes.
- A scene's blocks have no light, so a block whose animate tick reads light behaves as
  it would in the dark.
- Of the game's non-living entities, only dropped items and minecarts are drawn in scenes and
  screens; other types (boats, paintings, arrows) are refused as unknown. A minecart is drawn
  without its rail, so it never tilts on a slope.
