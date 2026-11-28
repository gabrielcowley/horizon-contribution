//! A general row as a vehicle (`gap/general-entity-sync`, riders): where its
//! riders sit, where they get off, and the mods' say in both.
//!
//! A rider's place is asked of the vehicle's parts once, when it is seated
//! (`general_rider_placement`), and again only when its box or vehicle
//! attachment changes or the vehicle's placements no longer name it. The
//! answer is kept in the vehicle's engine attachment `horizon:rider_places`,
//! which reaches the vehicle's trackers on the attachment paths, so both
//! sides then place the rider each tick by plain math
//! (`RiderPlacement::seat`). A rider that gets off asks
//! `general_dismount_location` once, unless the vehicle is removed or in a
//! portal block (vanilla's `LivingEntity.dismountVehicle`), and the vehicle's
//! parts hear `general_passenger_removed` once per rider and ride.

use super::super::Game;
use super::super::commands::Target;
use super::general_type_catalog;
use crate::game::entity::Entity;
use horizon_core::fast_hash::FxHashSet;
use horizon_core::geometry::Vec3;
use horizon_core::nbt::{Compound, Tag};
use mod_api::{
    AttachmentCodec, AttachmentError, AttachmentType, EntityRef, Holder, PassengerRemoval,
    RiderInfo, RiderPlacement,
};

/// One rider's kept place on a general vehicle: who, what it was asked
/// with (its box width and height and its vehicle attachment's height), and
/// the answer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::game) struct RiderPlace {
    pub rider: u32,
    pub asked: [f64; 3],
    pub placement: RiderPlacement,
}

/// The engine's rider placement attachment of a general vehicle: one entry
/// per seated rider, `[{Rider, Asked: [w, h, a], Local: [x, y, z], World: [x, y, z]}]`
/// under `Places`. Synced, so a tracker places the riders as the server
/// does; dropped when the row loads, since riders' ids are runtime ids.
pub(in crate::game) static RIDER_PLACES: AttachmentType<Vec<RiderPlace>> = AttachmentType {
    namespace: "horizon",
    name: "rider_places",
    default: Vec::new,
    codec: Some(AttachmentCodec {
        encode: encode_places,
        decode: decode_places,
    }),
    copy_on_death: false,
    sync: true,
};

fn doubles(values: &[f64]) -> Tag {
    Tag::List(values.iter().map(|v| Tag::Double(v.to_bits())).collect())
}

fn read_doubles<const N: usize>(tag: Option<&Tag>) -> Result<[f64; N], AttachmentError> {
    let Some(Tag::List(values)) = tag else {
        return Err(AttachmentError::Decode);
    };
    if values.len() != N {
        return Err(AttachmentError::Decode);
    }
    let mut out = [0.0; N];
    for (slot, value) in values.iter().enumerate() {
        let Tag::Double(bits) = value else {
            return Err(AttachmentError::Decode);
        };
        out[slot] = f64::from_bits(*bits);
        if !out[slot].is_finite() {
            return Err(AttachmentError::Decode);
        }
    }
    Ok(out)
}

fn encode_places(places: &Vec<RiderPlace>) -> Result<Compound, AttachmentError> {
    let list = places
        .iter()
        .map(|place| {
            let mut entry = Compound::new();
            entry.insert("Rider".into(), Tag::Int(place.rider as i32));
            entry.insert("Asked".into(), doubles(&place.asked));
            entry.insert("Local".into(), doubles(&place.placement.local));
            entry.insert("World".into(), doubles(&place.placement.world));
            Tag::Compound(entry)
        })
        .collect();
    let mut out = Compound::new();
    out.insert("Places".into(), Tag::List(list));
    Ok(out)
}

fn decode_places(saved: &Compound) -> Result<Vec<RiderPlace>, AttachmentError> {
    let Some(Tag::List(list)) = saved.get(&"Places".into()) else {
        return Err(AttachmentError::Decode);
    };
    list.iter()
        .map(|entry| {
            let Tag::Compound(entry) = entry else {
                return Err(AttachmentError::Decode);
            };
            let Some(Tag::Int(rider)) = entry.get(&"Rider".into()) else {
                return Err(AttachmentError::Decode);
            };
            Ok(RiderPlace {
                rider: *rider as u32,
                asked: read_doubles(entry.get(&"Asked".into()))?,
                placement: RiderPlacement::new(
                    read_doubles(entry.get(&"Local".into()))?,
                    read_doubles(entry.get(&"World".into()))?,
                ),
            })
        })
        .collect()
}

/// Vanilla's `Entity.positionRider` for a vehicle that overrides nothing:
/// the passenger attachment `(0, box height, 0)`, less the rider's own
/// vehicle attachment point.
pub(in crate::game) fn default_placement(box_height: f64, attachment: [f64; 3]) -> RiderPlacement {
    RiderPlacement::new(
        [0.0, box_height, 0.0],
        [-attachment[0], -attachment[1], -attachment[2]],
    )
}

impl Game<'_> {
    /// The general row `id` of the level in place, on either side.
    fn general_row(&self, id: u32) -> Option<&super::GeneralRow> {
        self.resident_entities
            .general
            .rows
            .iter()
            .find(|row| row.id == id)
    }

    /// Whether `id` is a general row of the level in place, on either side.
    pub(in crate::game) fn is_general_row(&self, id: u32) -> bool {
        self.general_row(id).is_some()
    }

    /// The general row `id` as a generic seat: its position, its box's height
    /// and its box's width. None when `id` is no general row here.
    pub(in crate::game) fn general_seat(&self, id: u32) -> Option<super::super::seats::Seat> {
        let row = self.general_row(id)?;
        let bounds = row.bounds();
        Some(super::super::seats::Seat {
            position: row.position,
            height: bounds.max_position().y - bounds.min_position().y,
            width: (bounds.max_position().x - bounds.min_position().x) as f32,
        })
    }

    /// Where `rider`, whose vehicle attachment point is `attachment` high,
    /// sits on the general row `vehicle` now: the kept placement when the
    /// vehicle has one for it, vanilla's default otherwise. None when
    /// `vehicle` is no general row here.
    pub(in crate::game) fn general_rider_seat(
        &self,
        vehicle: u32,
        rider: u32,
        attachment: f64,
    ) -> Option<Vec3> {
        let row = self.general_row(vehicle)?;
        let placement = self.general_rider_placement_of(vehicle, rider, attachment)?;
        let at = placement.seat(
            [row.position.x, row.position.y, row.position.z],
            row.rotation[0],
        );
        Some(Vec3::new(at[0], at[1], at[2]))
    }

    /// The same seat where the vehicle stood when its last tick began, for a
    /// frame's interpolation.
    pub(in crate::game) fn general_rider_previous_seat(
        &self,
        vehicle: u32,
        rider: u32,
        attachment: f64,
    ) -> Option<Vec3> {
        let row = self.general_row(vehicle)?;
        let placement = self.general_rider_placement_of(vehicle, rider, attachment)?;
        let at = placement.seat(
            [row.previous.x, row.previous.y, row.previous.z],
            row.previous_rotation[0],
        );
        Some(Vec3::new(at[0], at[1], at[2]))
    }

    fn general_rider_placement_of(
        &self,
        vehicle: u32,
        rider: u32,
        attachment: f64,
    ) -> Option<RiderPlacement> {
        let row = self.general_row(vehicle)?;
        if let Some(place) = self
            .resident_entities
            .general
            .rider_places
            .get(&vehicle)
            .and_then(|places| places.iter().find(|place| place.rider == rider))
        {
            return Some(place.placement);
        }
        let bounds = row.bounds();
        Some(default_placement(
            bounds.max_position().y - bounds.min_position().y,
            [0.0, attachment, 0.0],
        ))
    }

    /// What `general_rider_placement` is told about `rider` on a vehicle
    /// `vehicle_width` wide: its type, box and vehicle attachment point.
    fn general_rider_info(&self, rider: u32, vehicle_width: f32) -> Option<(String, [f64; 3])> {
        if let Some(mob) = self.mob_object(rider).filter(|mob| !mob.removed) {
            let attachment = horizon_client::mobs::riders::vehicle_attachment(mob, vehicle_width);
            return Some((
                mob.kind.id.to_string(),
                [f64::from(mob.width()), f64::from(mob.height()), attachment],
            ));
        }
        if let Some(row) = self.general_row(rider) {
            let bounds = row.bounds();
            return Some((
                general_type_catalog().key(row.type_slot).to_owned(),
                [
                    bounds.max_position().x - bounds.min_position().x,
                    bounds.max_position().y - bounds.min_position().y,
                    0.0,
                ],
            ));
        }
        // A player: `Avatar.DEFAULT_VEHICLE_ATTACHMENT`, scaled with its box.
        let entity = self.entity(rider)?;
        if !matches!(
            self.native_entity_target(entity)?,
            Target::Player | Target::Guest(_)
        ) {
            return None;
        }
        let [min, max] = self.entity_bounding_box(entity)?;
        let (width, height) = (max[0] - min[0], max[1] - min[1]);
        let standing = f64::from(horizon_client::pose::Pose::Standing.dimensions(1.0).width);
        Some((
            "minecraft:player".to_owned(),
            [width, height, 0.6 * width / standing],
        ))
    }

    /// Asks the vehicle's parts where `rider` sits and keeps the answer:
    /// `general_rider_placement`, from vanilla's default. A part that panics
    /// leaves the default. Server-side; nothing for a vehicle that is no
    /// general row or a rider the engine cannot describe.
    pub(in crate::game) fn seat_general_rider(&mut self, vehicle: u32, rider: u32) {
        if self.remote.is_some() {
            return;
        }
        let Some(seat) = self.general_seat(vehicle) else {
            return;
        };
        let Some((kind, asked)) = self.general_rider_info(rider, seat.width) else {
            return;
        };
        let (Some(epoch), Some(slot)) = (
            self.native_lifecycle.epoch(),
            self.general_row(vehicle).map(|row| row.type_slot),
        ) else {
            return;
        };
        let attachment = [0.0, asked[2], 0.0];
        let mut placement = default_placement(seat.height, attachment);
        let info = RiderInfo::new(
            EntityRef::new(epoch, rider),
            kind,
            asked[0],
            asked[1],
            attachment,
        );
        let vehicle_kind = general_type_catalog().key(slot);
        let default = placement;
        let asked_ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.native_general_rider_placement(
                vehicle_kind,
                EntityRef::new(epoch, vehicle),
                info,
                &mut placement,
            );
        }));
        if asked_ok.is_err()
            || !placement
                .local
                .iter()
                .chain(placement.world.iter())
                .all(|v| v.is_finite())
        {
            log::error!(
                "general entity {vehicle_kind} #{vehicle}: its rider placement failed; #{rider} sits at the default"
            );
            placement = default;
        }
        let mut places = self
            .resident_entities
            .general
            .rider_places
            .get(&vehicle)
            .cloned()
            .unwrap_or_default();
        places.retain(|place| place.rider != rider);
        places.push(RiderPlace {
            rider,
            asked,
            placement,
        });
        self.write_rider_places(vehicle, places);
    }

    /// Keeps `places` as the vehicle's, in its row's copy and its synced
    /// attachment.
    fn write_rider_places(&mut self, vehicle: u32, places: Vec<super::riders::RiderPlace>) {
        let holder = Holder::Entity(vehicle);
        let written = if places.is_empty() {
            RIDER_PLACES.remove(self, holder).map(|_| ())
        } else {
            RIDER_PLACES.set(self, holder, places.clone())
        };
        if written.is_err() {
            log::debug!("general entity #{vehicle}: its rider places were not written");
        }
        let store = &mut self.resident_entities.general.rider_places;
        if places.is_empty() {
            store.remove(&vehicle);
        } else {
            store.insert(vehicle, places);
        }
    }

    /// Client side: a general row's rider places follow its synced
    /// attachment, read again whenever the server sends that holder's
    /// attachments.
    pub(in crate::game) fn refresh_rider_places(&mut self, id: u32) {
        if !self.is_general_row(id) {
            return;
        }
        let places = RIDER_PLACES
            .get_existing(self, Holder::Entity(id))
            .ok()
            .flatten()
            .unwrap_or_default();
        let store = &mut self.resident_entities.general.rider_places;
        if places.is_empty() {
            store.remove(&id);
        } else {
            store.insert(id, places);
        }
    }

    /// A row that loads from a save keeps no rider places: its riders come
    /// back under new ids and are asked again.
    pub(in crate::game) fn forget_saved_rider_places(&mut self, id: u32) {
        let _ = RIDER_PLACES.remove(self, Holder::Entity(id));
        self.resident_entities.general.rider_places.remove(&id);
    }

    /// The riders of the general row `vehicle`, players and mobs first in
    /// boarding order, then general rows.
    fn general_riders_of(&self, vehicle: u32) -> Vec<u32> {
        self.seated_passengers(vehicle)
            .into_iter()
            .map(|(_, id, _)| id)
            .collect()
    }

    /// Once per server tick, after the entity phase: every rider of a
    /// general vehicle has a kept place asked with its present box and
    /// attachment, and a rider that left without getting off through the
    /// engine (removed, dead, gone to another level) is let go with
    /// `general_passenger_removed`. Asks no part while nothing changed.
    pub(in crate::game) fn reconcile_general_seating(&mut self) {
        if self.remote.is_some() || self.resident_entities.general.rows.is_empty() {
            return;
        }
        let general: FxHashSet<u32> = self
            .resident_entities
            .general
            .rows
            .iter()
            .map(|row| row.id)
            .collect();
        let mut vehicles: Vec<u32> = self
            .resident_entities
            .general
            .rows
            .iter()
            .filter_map(|row| row.mount)
            .chain(self.mob_objects().filter_map(|mob| mob.extra.mount))
            .chain(self.riding)
            .chain(
                self.lan
                    .iter()
                    .flat_map(|host| host.hub.guests.iter())
                    .filter_map(|guest| guest.vehicle),
            )
            .chain(self.resident_entities.general.rider_places.keys().copied())
            .filter(|id| general.contains(id))
            .collect();
        vehicles.sort_unstable();
        vehicles.dedup();
        for vehicle in vehicles {
            let riders = self.general_riders_of(vehicle);
            let kept = self
                .resident_entities
                .general
                .rider_places
                .get(&vehicle)
                .cloned()
                .unwrap_or_default();
            for place in &kept {
                if !riders.contains(&place.rider) {
                    self.general_rider_left(vehicle, place.rider, PassengerRemoval::RiderGone);
                }
            }
            let Some(width) = self.general_seat(vehicle).map(|seat| seat.width) else {
                continue;
            };
            for rider in riders {
                let asked = self
                    .general_rider_info(rider, width)
                    .map(|(_, asked)| asked);
                let current = kept.iter().find(|place| place.rider == rider);
                if asked.is_some() && current.map(|place| place.asked) != asked {
                    self.seat_general_rider(vehicle, rider);
                }
            }
        }
        // Rows that left keep no places.
        self.resident_entities
            .general
            .rider_places
            .retain(|vehicle, _| general.contains(vehicle));
    }

    /// `rider` is off the general row `vehicle` for `reason`: its kept place
    /// goes, and the vehicle's parts hear `general_passenger_removed`, once.
    /// Nothing when the vehicle kept no place for it, or on a client.
    pub(in crate::game) fn general_rider_left(
        &mut self,
        vehicle: u32,
        rider: u32,
        reason: PassengerRemoval,
    ) {
        if self.remote.is_some() {
            return;
        }
        let Some(mut places) = self
            .resident_entities
            .general
            .rider_places
            .get(&vehicle)
            .cloned()
        else {
            return;
        };
        let before = places.len();
        places.retain(|place| place.rider != rider);
        if places.len() == before {
            return;
        }
        if self.is_general_row(vehicle) {
            self.write_rider_places(vehicle, places);
        } else {
            self.resident_entities
                .general
                .rider_places
                .insert(vehicle, places);
        }
        let (Some(epoch), Some(slot)) = (
            self.native_lifecycle.epoch(),
            self.general_row(vehicle).map(|row| row.type_slot),
        ) else {
            return;
        };
        let kind = general_type_catalog().key(slot);
        let told = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.native_general_passenger_removed(
                kind,
                EntityRef::new(epoch, vehicle),
                EntityRef::new(epoch, rider),
                reason,
            );
        }));
        if told.is_err() {
            log::error!(
                "general entity {kind} #{vehicle}: a part panicked hearing #{rider} get off"
            );
        }
    }

    /// Why a rider getting off the general row `vehicle` now gets off.
    pub(in crate::game) fn general_removal_reason(&self, vehicle: u32) -> PassengerRemoval {
        if self.persist.removals.contains(&Target::General(vehicle)) {
            PassengerRemoval::VehicleRemoved
        } else {
            PassengerRemoval::Dismounted
        }
    }

    /// Whether the general row `vehicle` is removed or stands in a portal
    /// block, so its riders get off by `dismountVehicle`'s own rule (at their
    /// own x and z, at the higher y) and its parts are not asked: the
    /// vehicle's height when so.
    pub(in crate::game) fn general_vehicle_gone_y(&self, vehicle: u32) -> Option<f64> {
        let row = self.general_row(vehicle)?;
        if self.persist.removals.contains(&Target::General(vehicle)) {
            return Some(row.position.y);
        }
        let at = row.position;
        let block = self.res.facts().get(self.world.block(
            at.x.floor() as i32,
            at.y.floor() as i32,
            at.z.floor() as i32,
        ));
        self.named_block_in_tag("minecraft:portals", &block.name)
            .then_some(row.position.y)
    }

    /// vanilla's `Entity.getDismountLocationForPassenger` default for the
    /// general row `vehicle`: its x and z at the top of its box.
    pub(in crate::game) fn general_default_dismount(&self, vehicle: u32) -> Option<Vec3> {
        let row = self.general_row(vehicle)?;
        Some(Vec3::new(
            row.position.x,
            row.bounds().max_position().y,
            row.position.z,
        ))
    }

    /// Where `rider` gets off the general row `vehicle`, which is still in
    /// the level: `general_dismount_location`, from vanilla's default. A part
    /// that panics or answers a position that is not finite leaves the
    /// default. On a client, the default.
    pub(in crate::game) fn general_dismount_spot(
        &mut self,
        vehicle: u32,
        rider: u32,
    ) -> Option<Vec3> {
        let default = self.general_default_dismount(vehicle)?;
        if self.remote.is_some() {
            return Some(default);
        }
        let (Some(epoch), Some(slot)) = (
            self.native_lifecycle.epoch(),
            self.general_row(vehicle).map(|row| row.type_slot),
        ) else {
            return Some(default);
        };
        let kind = general_type_catalog().key(slot);
        let mut location = [default.x, default.y, default.z];
        let asked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.native_general_dismount_location(
                kind,
                EntityRef::new(epoch, vehicle),
                EntityRef::new(epoch, rider),
                &mut location,
            );
        }));
        if asked.is_err() || !location.iter().all(|v| v.is_finite()) {
            log::error!(
                "general entity {kind} #{vehicle}: its dismount location failed; #{rider} gets off at the default"
            );
            return Some(default);
        }
        Some(Vec3::new(location[0], location[1], location[2]))
    }

    /// Where a rider at `position`, `w` wide and `h` tall, gets off the
    /// general row `vehicle` (`LivingEntity.dismountVehicle`): by the
    /// removed vehicle's rule when it is removed or in a portal block,
    /// otherwise where `general_dismount_location` says.
    pub(in crate::game) fn general_dismount_point(
        &mut self,
        vehicle: u32,
        rider: u32,
        position: Vec3,
        w: f64,
        h: f64,
    ) -> Option<Vec3> {
        if let Some(y) = self.general_vehicle_gone_y(vehicle) {
            return Some(self.removed_vehicle_dismount_point(position, w, h, y));
        }
        self.general_dismount_spot(vehicle, rider)
    }

    /// The general riders of the general row `vehicle`, removed and being
    /// swept, get off by the removed vehicle's rule (at their own x and z, at
    /// the higher y) and the vehicle's parts hear `VehicleRemoved`.
    pub(in crate::game) fn eject_removed_general_riders(&mut self, vehicle: u32) {
        let Some(vehicle_y) = self.general_row(vehicle).map(|row| row.position.y) else {
            return;
        };
        let riders: Vec<(usize, u32)> = self
            .resident_entities
            .general
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.mount == Some(vehicle))
            .map(|(index, row)| (index, row.id))
            .collect();
        for (index, id) in riders {
            let row = &self.resident_entities.general.rows[index];
            let bounds = row.bounds();
            let at = self.removed_vehicle_dismount_point(
                row.position,
                bounds.max_position().x - bounds.min_position().x,
                bounds.max_position().y - bounds.min_position().y,
                vehicle_y,
            );
            self.resident_entities.general.rows.update(index, |row| {
                row.mount = None;
                row.position = at;
                row.previous = at;
            });
            self.resident_entities.general.mount_writes += 1;
            self.general_rider_left(vehicle, id, PassengerRemoval::VehicleRemoved);
        }
    }

    /// A general row's riders that are general rows sit where their
    /// vehicle's placement puts them (`Entity.positionRider`), on both sides.
    pub(in crate::game) fn position_general_rider(&mut self, index: usize) {
        let row = &self.resident_entities.general.rows[index];
        let (id, Some(vehicle)) = (row.id, row.mount) else {
            return;
        };
        let Some(at) = self.general_rider_seat(vehicle, id, 0.0) else {
            return;
        };
        let rows = &mut self.resident_entities.general.rows;
        rows.update(index, |row| {
            row.position = at;
            row.velocity = Vec3::ZERO;
            row.fall_distance = 0.0;
        });
    }

    /// A client's general riders sit on their vehicles after the rows eased
    /// toward the server's poses: a passenger's place is its vehicle's.
    pub(in crate::game) fn position_remote_general_riders(&mut self) {
        // Riders of riders settle over a few passes, vehicle before rider.
        for _ in 0..3 {
            for index in 0..self.resident_entities.general.rows.len() {
                if self.resident_entities.general.rows[index].mount.is_some() {
                    self.position_general_rider(index);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rider_places_round_trip_through_their_codec() {
        let places = vec![RiderPlace {
            rider: 41,
            asked: [0.6, 1.8, 0.6],
            placement: RiderPlacement::new([0.0, 0.4125, 0.0], [0.0, -0.6, 0.0]),
        }];
        let tag = encode_places(&places).unwrap();
        assert_eq!(decode_places(&tag).unwrap(), places);
    }

    #[test]
    fn the_default_placement_is_a_generic_seats() {
        // A display-like row 0 tall seats a player 0.6 below its position, as
        // `Seat::player_seat` does; the 1.975-tall armor stand's height
        // seats it at 1.375.
        let display = default_placement(0.0, [0.0, 0.6, 0.0]).seat([1.5, 64.0, -2.5], 37.0);
        assert!((display[0] - 1.5).abs() < 1e-12 && (display[2] - -2.5).abs() < 1e-12);
        assert!((display[1] - 63.4).abs() < 1e-12);
        let stand = default_placement(1.975, [0.0, 0.6, 0.0]).seat([0.5, 64.0, 0.5], 0.0);
        assert!((stand[1] - 65.375).abs() < 1e-9);
    }
}
