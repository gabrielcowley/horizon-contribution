//! A general row's own box and the spawn call that gives one
//! (`gap/general-entity-sync`): `entity_set_bounds` writes a box relative to
//! the row's position, which every box read (`entity_bounding_box`,
//! `level_entities_in`, culling, the debug hitbox) then takes. The value is
//! the engine's own synced attachment, so it saves with the row, reaches a
//! client that starts tracking it in the attachment snapshot, and reaches
//! the others as a delta, all on the paths every attachment takes.

use super::super::Game;
use super::{GeneralRow, general_type_catalog, limits};
use horizon_core::nbt::{Compound, Tag};
use mod_api::{AttachmentCodec, AttachmentError, AttachmentType, EntityRef, Holder};

/// A general row's box relative to its position: `[min, max]`.
pub(in crate::game) type LocalBox = [[f64; 3]; 2];

/// The engine's box attachment: `{Min: [x, y, z], Max: [x, y, z]}`.
pub(in crate::game) static GENERAL_BOX: AttachmentType<LocalBox> = AttachmentType {
    namespace: "horizon",
    name: "general_box",
    default: || [[0.0; 3]; 2],
    codec: Some(AttachmentCodec {
        encode: encode_box,
        decode: decode_box,
    }),
    copy_on_death: false,
    sync: true,
};

fn encode_box(value: &LocalBox) -> Result<Compound, AttachmentError> {
    let mut out = Compound::new();
    for (key, corner) in [("Min", value[0]), ("Max", value[1])] {
        out.insert(
            key.into(),
            Tag::List(corner.map(|v| Tag::Double(v.to_bits())).to_vec()),
        );
    }
    Ok(out)
}

fn decode_box(saved: &Compound) -> Result<LocalBox, AttachmentError> {
    let corner = |key: &str| -> Result<[f64; 3], AttachmentError> {
        let Some(Tag::List(values)) = saved.get(&key.into()) else {
            return Err(AttachmentError::Decode);
        };
        let mut out = [0.0; 3];
        if values.len() != 3 {
            return Err(AttachmentError::Decode);
        }
        for (slot, value) in values.iter().enumerate() {
            let Tag::Double(bits) = value else {
                return Err(AttachmentError::Decode);
            };
            out[slot] = f64::from_bits(*bits);
        }
        Ok(out)
    };
    let local = [corner("Min")?, corner("Max")?];
    box_ok(local)
        .then_some(local)
        .ok_or(AttachmentError::Decode)
}

/// Whether a box may be a row's: every corner finite, `min <= max` on each
/// axis, and no side longer than [`limits::MAX_BOX_EXTENT`].
pub(in crate::game) fn box_ok([min, max]: LocalBox) -> bool {
    (0..3).all(|axis| {
        min[axis].is_finite()
            && max[axis].is_finite()
            && min[axis] <= max[axis]
            && max[axis] - min[axis] <= limits::MAX_BOX_EXTENT
    })
}

impl Game<'_> {
    /// The box a row's saved or synced attachments carry, if any.
    pub(in crate::game) fn saved_general_box(&mut self, id: u32) -> Option<LocalBox> {
        GENERAL_BOX
            .get_existing(self, Holder::Entity(id))
            .ok()
            .flatten()
    }

    /// The general row `id`'s place in its level's list.
    fn general_row_index(&self, id: u32) -> Option<usize> {
        self.resident_entities
            .general
            .rows
            .iter()
            .position(|row: &GeneralRow| row.id == id)
    }

    /// `entity_set_bounds`: the general row's box becomes `min..max`
    /// relative to its position. False, and nothing written, for an entity
    /// that is not a general row of the level in place, a box that is not
    /// finite, inverted or longer than 64 blocks on a side, or on a client.
    pub(in crate::game) fn entity_set_bounds(
        &mut self,
        entity: EntityRef,
        min: [f64; 3],
        max: [f64; 3],
    ) -> bool {
        let local = [min, max];
        if self.remote.is_some() || !box_ok(local) || self.native_entity_target(entity).is_none() {
            return false;
        }
        let Some(index) = self.general_row_index(entity.id()) else {
            return false;
        };
        if GENERAL_BOX
            .set(self, Holder::Entity(entity.id()), local)
            .is_err()
        {
            return false;
        }
        self.resident_entities.general.rows[index].local_box = Some(local);
        true
    }

    /// Client side: a general row's box follows its synced attachment, read
    /// again whenever the server sends that holder's attachments.
    pub(in crate::game) fn refresh_general_box(&mut self, id: u32) {
        let Some(index) = self.general_row_index(id) else {
            return;
        };
        let local = self.saved_general_box(id);
        self.resident_entities.general.rows[index].local_box = local;
    }

    /// `spawn_general`: a general row of the declared type `kind` at `pos`,
    /// turned to `rotation` (yaw, pitch), with its own box when `bounds` is
    /// given, through the path `summon` takes, so inside a transaction its
    /// join waits for the root commit while the handle comes back at once.
    /// The caller writes its attachments on the handle before the tick ends,
    /// and a client that starts tracking the row gets them in its spawn
    /// snapshot. None for a kind no mod declared as a general type, a box
    /// `entity_set_bounds` would refuse, or whatever `summon` refuses.
    pub(in crate::game) fn spawn_general(
        &mut self,
        dimension: &str,
        kind: &str,
        pos: [f64; 3],
        rotation: [f32; 2],
        bounds: Option<[[f64; 3]; 2]>,
    ) -> Option<EntityRef> {
        let slot = general_type_catalog().slot(kind)?;
        if bounds.is_some_and(|local| !box_ok(local)) || !rotation.iter().all(|r| r.is_finite()) {
            return None;
        }
        let mut tag = Compound::new();
        tag.insert(
            "Rotation".into(),
            Tag::List(rotation.map(|r| Tag::Float(r.to_bits())).to_vec()),
        );
        tag.insert(
            "UUID".into(),
            Tag::IntArray(crate::save::random_uuid().to_vec()),
        );
        tag.insert(
            "PayloadVersion".into(),
            Tag::Short(i16::from_ne_bytes(
                general_type_catalog().payload_version(slot).to_ne_bytes(),
            )),
        );
        if let Some(local) = bounds {
            // The box goes in as a saved attachment, so the row is born with
            // it and no tick ever sees the declared one.
            let mut value = Compound::new();
            value.insert("value".into(), Tag::Compound(encode_box(&local).ok()?));
            value.insert("sync".into(), Tag::Byte(1));
            let mut names = Compound::new();
            names.insert(GENERAL_BOX.name.into(), Tag::Compound(value));
            let mut namespaces = Compound::new();
            namespaces.insert(GENERAL_BOX.namespace.into(), Tag::Compound(names));
            tag.insert(
                horizon_mod_runtime::attachments::ATTACHMENTS_KEY.into(),
                Tag::Compound(namespaces),
            );
        }
        let id = self.summon(dimension, kind, pos, tag)?;
        Some(EntityRef::new(self.native_lifecycle.epoch()?, id))
    }
}
