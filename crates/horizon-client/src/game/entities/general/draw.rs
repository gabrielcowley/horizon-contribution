//! A client's general rows drawn (`gap/general-entity-sync`): each row whose type a mod gave a
//! renderer (`register_entity_renderers`) is handed an [`EntityRenderState`] built from its client
//! copy alone (its type, box and synced attachment values, `GeneralRenderState`), with its shadow;
//! a type with no renderer draws nothing but its debug hitbox, as vanilla's `NoopRenderer` does.
use super::super::Game;
use super::{GeneralRow, general_type_catalog};
use horizon_client::mesher::Vertex;
use mod_api::{EntityRenderState, GeneralRenderState, Holder};

impl Game<'_> {
    /// The row's box relative to its position: its own, or its type's declared one.
    pub(in crate::game) fn general_local_box(row: &GeneralRow) -> [[f64; 3]; 2] {
        row.local_box.unwrap_or_else(|| {
            let (width, height) = general_type_catalog().dimensions(row.type_slot);
            let half = width / 2.0;
            [[-half, 0.0, -half], [half, height, half]]
        })
    }

    /// The row's frame at the partial tick `alpha`, built from what its client copy holds.
    pub(in crate::game) fn general_render_state(
        &self,
        row: &GeneralRow,
        alpha: f32,
    ) -> EntityRenderState {
        let mut attachments = mod_api::Compound::new();
        let synced = self
            .remote
            .as_ref()
            .and_then(|r| r.attachments.get(&Holder::Entity(row.id)))
            .map(|map| map.synced())
            .unwrap_or_default();
        for value in synced {
            let Some(encoded) = value.value else { continue };
            let names = attachments
                .entry(value.namespace)
                .or_insert_with(|| mod_api::Tag::Compound(mod_api::Compound::new()));
            if let mod_api::Tag::Compound(names) = names {
                names.insert(value.name, mod_api::Tag::Compound(encoded));
            }
        }
        let mut general = GeneralRenderState::new(
            general_type_catalog().key(row.type_slot),
            Self::general_local_box(row),
        );
        general.attachments = attachments;
        let lerp = |from: f32, to: f32| from + alpha * (to - from);
        EntityRenderState::of_general(
            row.id,
            alpha,
            row.age as f32 + alpha,
            lerp(row.previous_rotation[0], row.rotation[0]),
            lerp(row.previous_rotation[1], row.rotation[1]),
            general,
        )
    }

    /// The row's position at the partial tick `alpha`.
    pub(in crate::game) fn general_frame_position(row: &GeneralRow, alpha: f64) -> [f64; 3] {
        let (from, to) = (row.previous, row.position);
        [
            from.x + (to.x - from.x) * alpha,
            from.y + (to.y - from.y) * alpha,
            from.z + (to.z - from.z) * alpha,
        ]
    }

    /// Draws every general row a renderer draws, relative to `camera` at the partial tick
    /// `alpha`.
    pub(in crate::game) fn general_geometry(
        &self,
        camera: glam::DVec3,
        alpha: f64,
        out: &mut Vec<Vertex>,
    ) {
        for row in self.resident_entities.general.rows.iter() {
            let Some((resolved, _)) =
                crate::game::native_entity_renderers::general_renderer(row.type_slot)
            else {
                continue;
            };
            let at = Self::general_frame_position(row, alpha);
            let local = Self::general_local_box(row);
            // `Entity.getLightProbePosition`: its eyes, 0.85 of its height up.
            let probe = at[1] + local[0][1] + (local[1][1] - local[0][1]) * 0.85;
            let light = self.world.light(
                at[0].floor() as i32,
                probe.floor() as i32,
                at[2].floor() as i32,
            );
            let packed = (u32::from(light >> 4) * 16) | ((u32::from(light & 15) * 16) << 8);
            let state = self.general_render_state(row, alpha as f32);
            let start = out.len();
            if self.mod_entity_renderer_geometry(
                &resolved,
                &state,
                glam::DVec3::from_array(at) - camera,
                at.map(|x| x.floor() as i32),
                packed,
                out,
            ) {
                self.mark_entity(start, general_type_catalog().key(row.type_slot));
            }
        }
    }

    /// The shadows of the general rows whose renderer asks for one: `(position, radius)`.
    pub(in crate::game) fn general_shadow_casters(&self, alpha: f64) -> Vec<(glam::DVec3, f32)> {
        self.resident_entities
            .general
            .rows
            .iter()
            .filter_map(|row| {
                let (_, radius) =
                    crate::game::native_entity_renderers::general_renderer(row.type_slot)?;
                (radius > 0.0).then(|| {
                    (
                        glam::DVec3::from_array(Self::general_frame_position(row, alpha)),
                        radius,
                    )
                })
            })
            .collect()
    }
}
