//! Code-defined entity renderers, as a client mod registers them in `register_entity_renderers`
//! (NeoForge's `EntityRenderersEvent.RegisterRenderers` and `EntityRenderer`).
//!
//! A renderer draws every entity of one entity type, named by its id (`mymod:bolt`). The game
//! keeps the registrations from start and resolves each type's renderer on every resource load, so
//! a frame hands an entity to the renderer of its type's slot and to no other mod.
use crate::BlockEntityDrawContext;
use crate::wire::{Reader, Wire, WireError, Writer};
use std::{collections::BTreeMap, sync::Arc};

/// One entity's frame, as `EntityRenderer.extractRenderState` fills an `EntityRenderState`:
/// what a renderer reads to place and turn its model. Angles are in degrees, interpolated to the
/// frame's partial tick. Plain values, and one value for native and wasm mods alike. Only a
/// general entity's frame ([`GeneralRenderState`]) allocates, for its attachment values.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, horizon_mod_api_macros::Wire)]
pub struct EntityRenderState {
    /// The entity's network id, as mod messages name it.
    pub id: u32,
    /// The frame's partial tick, 0 to 1.
    pub partial: f32,
    /// `ageInTicks`: ticks lived plus the partial tick, for motion of its own.
    pub age: f32,
    /// `Entity.getYRot` at the frame: the way it faces (0 is south, 90 west).
    pub yaw: f32,
    /// `Entity.getXRot` at the frame: its pitch, positive looking down.
    pub pitch: f32,
    /// A living entity's own frame (`LivingEntityRenderState`); none for any other entity.
    pub living: Option<LivingRenderState>,
    /// A general entity's own frame: its type, box and synced attachment values; none for any
    /// other entity.
    pub general: Option<GeneralRenderState>,
}

impl EntityRenderState {
    /// A general entity's frame built from data alone, with no entity in any world behind it:
    /// what a screen or a scene hands a general type's renderer to draw it the way the world
    /// does. `id` is whatever the caller names it by (0 when nothing does), `age` its ticks
    /// lived plus the partial tick.
    pub fn of_general(
        id: u32,
        partial: f32,
        age: f32,
        yaw: f32,
        pitch: f32,
        general: GeneralRenderState,
    ) -> Self {
        Self {
            id,
            partial,
            age,
            yaw,
            pitch,
            living: None,
            general: Some(general),
        }
    }
}

/// What a general entity's frame adds to [`EntityRenderState`]: everything a renderer of a mod's
/// own entity type reads beyond its pose. The game builds it from the entity's client copy, and a
/// mod can build one from data ([`GeneralRenderState::new`], [`with_attachment`](Self::with_attachment))
/// to draw the type where no entity is, in a screen or a scene. It never needs a live entity.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, horizon_mod_api_macros::Wire)]
pub struct GeneralRenderState {
    /// The entity's type (`mymod:robot`).
    pub type_key: String,
    /// Its box relative to its position, `[min, max]`: the one `entity_set_bounds` gave it, or
    /// its type's declared width and height.
    pub local_box: [[f64; 3]; 2],
    /// Its synced attachment values, encoded, by namespace then name; read them with
    /// [`attachment`](Self::attachment).
    pub attachments: crate::Compound,
}

impl GeneralRenderState {
    /// A frame of the type `type_key` with the box `local_box` and no attachment values.
    pub fn new(type_key: impl Into<String>, local_box: [[f64; 3]; 2]) -> Self {
        Self {
            type_key: type_key.into(),
            local_box,
            attachments: crate::Compound::new(),
        }
    }

    /// The frame with `value` as its value of the attachment `key`, encoded by its codec, as the
    /// server would have sent it. A key with no codec, or a value its codec refuses, leaves the
    /// frame without it.
    pub fn with_attachment<T: Clone + Send + Sync + 'static>(
        mut self,
        key: &crate::AttachmentType<T>,
        value: &T,
    ) -> Self {
        let Some(encoded) = key
            .codec
            .as_ref()
            .and_then(|codec| (codec.encode)(value).ok())
        else {
            return self;
        };
        let names = self
            .attachments
            .entry(key.namespace.into())
            .or_insert_with(|| crate::Tag::Compound(crate::Compound::new()));
        if let crate::Tag::Compound(names) = names {
            names.insert(key.name.into(), crate::Tag::Compound(encoded));
        }
        self
    }

    /// The frame's value of the synced attachment `key`, decoded by its codec: what the entity
    /// held when the server last sent it. None when it holds none, or for a key with no codec
    /// or a value that does not decode.
    pub fn attachment<T: Clone + Send + Sync + 'static>(
        &self,
        key: &crate::AttachmentType<T>,
    ) -> Option<T> {
        let Some(crate::Tag::Compound(names)) = self.attachments.get(&key.namespace.into()) else {
            return None;
        };
        let Some(crate::Tag::Compound(value)) = names.get(&key.name.into()) else {
            return None;
        };
        (key.codec.as_ref()?.decode)(value).ok()
    }
}

/// What a living entity's frame adds to [`EntityRenderState`], as
/// `LivingEntityRenderer.extractRenderState` fills a `LivingEntityRenderState`: what a model
/// reads to pose and animate a body. Angles are in degrees, interpolated to the frame's partial
/// tick. Plain values, so a frame allocates nothing, and one value for native and wasm mods alike.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, horizon_mod_api_macros::Wire)]
pub struct LivingRenderState {
    /// `bodyRot`: the way the body faces (0 is south, 90 west).
    pub body_rot: f32,
    /// `yRot`: the head's turn from the body, -180 to 180 (its sign flipped when
    /// `is_upside_down`).
    pub y_rot: f32,
    /// `xRot`: the head's pitch, positive looking down (its sign flipped when `is_upside_down`).
    pub x_rot: f32,
    /// `deathTime`: ticks since it died plus the partial tick, 0 while alive (vanilla tips the
    /// body over the first 20).
    pub death_time: f32,
    /// `walkAnimationPos`: how far the legs have swung, for a walk cycle (a vanilla quadruped's
    /// legs swing `cos(pos * 0.6662) * 1.4 * speed` radians); 0 riding or dead.
    pub walk_animation_pos: f32,
    /// `walkAnimationSpeed`: how fast it walks, 0 standing to 1; 0 riding or dead.
    pub walk_animation_speed: f32,
    /// `scale`: the entity's `minecraft:scale` (`LivingEntity.getScale`), by which vanilla scales
    /// the whole model.
    pub scale: f32,
    /// `ageScale`: 0.5 for a baby, else 1 (`LivingEntity.getAgeScale`).
    pub age_scale: f32,
    /// `isUpsideDown`: named Dinnerbone or Grumm, drawn head down.
    pub is_upside_down: bool,
    /// `isBaby`.
    pub is_baby: bool,
    /// `isInWater`.
    pub is_in_water: bool,
    /// `hasRedOverlay`: hurt or dying, drawn tinted red.
    pub has_red_overlay: bool,
    /// `pose`: its stance (`Entity.getPose`).
    pub pose: crate::Pose,
    /// `bedOrientation`: the way the bed it sleeps in faces, while it sleeps in one.
    pub bed_orientation: Option<crate::Direction>,
}

/// A mod's entity renderer: NeoForge's `EntityRenderer`, held by the game for every entity of the
/// type it is registered for. Entities of a type share one renderer, so what differs between them
/// comes from [`EntityRenderState`].
///
/// A frame has no `Host`: drawing reads nothing of the world but what the state and `draw` give.
/// A renderer that needs more reads it in a hook and keeps what it read.
pub trait EntityRenderer: Send + Sync + 'static {
    /// `EntityRenderer.submit`: draw `entity` with `draw`'s helpers (`quad`, `cube`, `block`,
    /// `item`, `model_part`) under its pose stack, which starts at the entity's position, not
    /// turned: turn it by `entity.yaw` and `entity.pitch` as the model needs. `draw` also has
    /// the partial tick, the game time and the entity's light. Asked once a frame for each entity
    /// of the type in view; what it draws replaces the type's own drawing. A frame is drawn
    /// afresh: `draw.keep()` does nothing here. A native implementation returns `Ok(())` in the
    /// normal case, and that keeps what it drew. Diverges: Horizon's `submit` returns a `Result`
    /// so a refused or trapped call is visible. An `Err`, a panic, and a wasm call that is
    /// refused or traps all mean the same: what it drew this frame is dropped, the error is
    /// logged once, and the renderer is asked no more until the next resource load resolves it
    /// again; until then its type draws as it would without one.
    fn submit(
        &self,
        entity: &EntityRenderState,
        draw: &mut dyn BlockEntityDrawContext,
    ) -> Result<(), String>;
}

crate::wire::wire_handlers::handler!(dyn EntityRenderer, "EntityRenderer");

/// The entity renderer registry `register_entity_renderers` hands out: NeoForge's
/// `EntityRenderersEvent.RegisterRenderers`.
pub trait EntityRenderers {
    /// `registerEntityRenderer(type, provider)`: `renderer` draws every entity of the type
    /// `entity_type` (`mymod:bolt`). A later registration for a type replaces the earlier one,
    /// whichever mod made it. A type the game does not know is logged with its id when resources
    /// load, and nothing is drawn by it.
    fn register(&mut self, entity_type: String, renderer: Arc<dyn EntityRenderer>);

    /// `EntityRenderer.shadowRadius` of the type `entity_type`'s renderer: the radius in blocks
    /// of the round shadow drawn under each entity of the type on the ground; 0, vanilla's
    /// default, for none. Read for a mod's general entity types, whose own drawing has no
    /// shadow; a radius that is not finite or below 0 counts as 0. A later call for a type
    /// replaces the earlier one.
    fn shadow_radius(&mut self, entity_type: String, radius: f32);
}

// no wire: process-local listener or callback storage; trait objects are invoked through host dispatch, not serialized.
/// The game's [`EntityRenderers`]: the registrations by entity type.
#[derive(Default)]
// exhaustive: its field is private; the game builds it
pub struct EntityRendererRegistry {
    renderers: BTreeMap<String, Arc<dyn EntityRenderer>>,
    shadows: BTreeMap<String, f32>,
}

impl EntityRenderers for EntityRendererRegistry {
    fn register(&mut self, entity_type: String, renderer: Arc<dyn EntityRenderer>) {
        self.renderers.insert(entity_type, renderer);
    }
    fn shadow_radius(&mut self, entity_type: String, radius: f32) {
        let radius = if radius.is_finite() && radius > 0.0 {
            radius
        } else {
            0.0
        };
        self.shadows.insert(entity_type, radius);
    }
}

impl EntityRendererRegistry {
    /// Each type's renderer, the last registration for a type winning.
    pub fn into_renderers(self) -> BTreeMap<String, Arc<dyn EntityRenderer>> {
        self.renderers
    }
    /// Each type's renderer and the shadow radius its mod gave it, the last call for a type
    /// winning.
    pub fn into_renderers_and_shadows(
        self,
    ) -> (
        BTreeMap<String, Arc<dyn EntityRenderer>>,
        BTreeMap<String, f32>,
    ) {
        (self.renderers, self.shadows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Marked(u32);
    impl EntityRenderer for Marked {
        fn submit(
            &self,
            _entity: &EntityRenderState,
            draw: &mut dyn BlockEntityDrawContext,
        ) -> Result<(), String> {
            draw.set_light(self.0);
            Ok(())
        }
    }

    #[derive(Default)]
    struct Light(u32);
    impl BlockEntityDrawContext for Light {
        fn set_light(&mut self, light: u32) {
            self.0 = light;
        }
    }

    #[test]
    fn a_later_registration_for_a_type_replaces_the_earlier_one() {
        let mut registry = EntityRendererRegistry::default();
        registry.register("mymod:bolt".into(), Arc::new(Marked(1)));
        registry.register("mymod:dart".into(), Arc::new(Marked(2)));
        registry.register("mymod:bolt".into(), Arc::new(Marked(3)));
        let renderers = registry.into_renderers();
        assert_eq!(renderers.len(), 2);
        let state = EntityRenderState {
            id: 7,
            partial: 0.5,
            age: 3.5,
            yaw: 0.0,
            pitch: 0.0,
            living: None,
            general: None,
        };
        let (mut bolt, mut dart) = (Light::default(), Light::default());
        renderers["mymod:bolt"].submit(&state, &mut bolt).unwrap();
        renderers["mymod:dart"].submit(&state, &mut dart).unwrap();
        assert_eq!((bolt.0, dart.0), (3, 2));
    }

    static GLOW: crate::AttachmentType<i32> = crate::AttachmentType {
        namespace: "mymod",
        name: "glow",
        default: || 0,
        codec: Some(crate::AttachmentCodec {
            encode: |n| Ok([("n".into(), crate::Tag::Int(*n))].into()),
            decode: |tag| match tag.get(&"n".into()) {
                Some(crate::Tag::Int(n)) => Ok(*n),
                _ => Err(crate::AttachmentError::Decode),
            },
        }),
        copy_on_death: false,
        sync: true,
    };

    /// A frame built from data alone carries the values it was built with to its renderer, as a
    /// world entity's synced ones do, and survives the trip a wasm renderer's state takes.
    #[test]
    fn a_general_frame_built_from_data_reads_its_attachment_back() {
        let general = GeneralRenderState::new("mymod:crate", [[-0.5, 0.0, -0.5], [0.5, 1.0, 0.5]])
            .with_attachment(&GLOW, &12);
        assert_eq!(general.attachment(&GLOW), Some(12));
        let state = EntityRenderState::of_general(0, 0.25, 3.25, 90.0, 0.0, general);
        let mut out = Writer::default();
        state.put(&mut out);
        let read = EntityRenderState::take(&mut Reader::new(out.bytes())).unwrap();
        let general = read.general.as_ref().unwrap();
        assert_eq!(general.attachment(&GLOW), Some(12));
        assert_eq!(general.local_box, [[-0.5, 0.0, -0.5], [0.5, 1.0, 0.5]]);
        assert_eq!(
            GeneralRenderState::new("mymod:crate", [[0.0; 3]; 2]).attachment(&GLOW),
            None
        );
    }
}
