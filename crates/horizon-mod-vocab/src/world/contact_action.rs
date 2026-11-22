//! Contact actions: what a block part queues while an entity's move touches it, and the
//! swept entity the game later hands back to it with a write host.
//!
//! During the sweep (`entity_inside`) and the landing (`entity_after_fall_on`) a part may
//! only read and queue. Each [`ContactAction`] it queues is the part's own typed value,
//! encoded with [`Wire`]. Inside the swept entity's own tick, once the entity's planned
//! writes are applied and before the next entity moves, the game calls the part's
//! `contact_action` with the server write host, in the order the moves queued them: a
//! depot inserts the item that landed on it and sees the leftover, so a second item
//! landing on a full depot the same tick stays whole.
use crate::ItemStack;
use crate::wire::{Reader, Wire, WireError, Writer};

/// The moment a [`ContactAction`] was queued from.
// exhaustive: the two contact moments that queue actions; a new moment is a new contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, horizon_mod_api_macros::Wire)]
pub enum ContactMoment {
    /// `Block.entityInside`: the entity's block sweep crossed the block.
    Inside,
    /// The entity's move ended on top of the block, after vanilla's landing response
    /// (`Entity.restituteMovementAfterCollisions`).
    AfterFallOn,
}

/// A part's own action, queued mid-move and called back in the swept entity's tick.
///
/// The bytes are the part's typed value, encoded with [`Wire`]: build one with
/// [`ContactAction::new`] and read it back with [`ContactAction::decode`]. Every part
/// attached to the touched block state that implements `contact_action` hears every
/// action queued at that block, so a part ignores one it cannot decode.
// exhaustive: the single public field is the opaque-byte constructor contract.
#[derive(Clone, Debug, Default, PartialEq, Eq, horizon_mod_api_macros::Wire)]
pub struct ContactAction(pub Vec<u8>);

impl ContactAction {
    /// The most bytes one action may carry; the game refuses a longer one when it is
    /// queued, as a part's other invalid output is refused.
    pub const MAX_BYTES: usize = 4096;

    /// An action carrying `value`, encoded with [`Wire`], or the error the encoder recorded.
    pub fn new<T: Wire>(value: &T) -> Result<Self, WireError> {
        let mut writer = Writer::new();
        value.put(&mut writer);
        match writer.error() {
            Some(error) => Err(error),
            None => Ok(Self(writer.into_bytes())),
        }
    }

    /// The typed value this action carries, or the error that says why it is not a `T`.
    /// The whole action must be read: trailing bytes are an error.
    pub fn decode<T: Wire>(&self) -> Result<T, WireError> {
        Reader::decode(&self.0)
    }

    /// Whether the game accepts this action: at most [`Self::MAX_BYTES`] bytes.
    pub fn is_valid(&self) -> bool {
        self.0.len() <= Self::MAX_BYTES
    }
}

/// The swept entity as a `contact_action` call sees it, writable.
///
/// The game fills it from the entity when it calls the action and writes the fields
/// back after the call: what the part changes here is what the entity keeps, in its own
/// tick, as vanilla's `entityInside` and landing response write the entity
/// directly. Non-finite positions or movements are not written back.
// exhaustive: the swept entity's writable record is a closed snapshot the game fills and reads back.
#[derive(Clone, Debug, Default, PartialEq, horizon_mod_api_macros::Wire)]
pub struct ContactEntity {
    /// The entity's id (`Entity.getId`).
    pub id: i32,
    /// The entity's type, `minecraft:item` for a dropped item.
    pub kind: String,
    /// Where the entity is (`Entity.position`): set it to move the entity there (`setPos`).
    pub position: [f64; 3],
    /// The entity's movement (`getDeltaMovement` / `setDeltaMovement`).
    pub movement: [f64; 3],
    /// Whether the entity stands on a block (`onGround` / `setOnGround`).
    pub on_ground: bool,
    /// Set to `makeStuckInBlock(factors)`: the movement is scaled by these factors as a
    /// cobweb scales it, and the fall distance is reset. `None` leaves it as it is.
    pub stuck_in_block: Option<[f64; 3]>,
    /// A dropped item's stack (`ItemEntity.getItem` / `setItem`): leave the leftover of
    /// an insert here. `None` (or a dropped item's stack set to `None`) takes the whole
    /// stack, and the item is discarded. Always `None` for any other entity, and ignored.
    pub item: Option<ItemStack>,
    /// Set to discard the entity (`Entity.discard`), as a part that takes it whole does.
    pub discard: bool,
}

impl ContactEntity {
    /// Whether the entity is a dropped item.
    pub fn is_item(&self) -> bool {
        self.kind == "minecraft:item"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_action_reads_back_the_value_it_was_built_from() {
        let action = ContactAction::new(&(7u32, String::from("belt"))).unwrap();
        assert_eq!(
            action.decode::<(u32, String)>().unwrap(),
            (7, "belt".to_owned())
        );
        assert!(action.is_valid());
    }

    #[test]
    fn an_action_with_bytes_left_over_is_not_that_type() {
        let action = ContactAction::new(&(7u32, 9u32)).unwrap();
        assert!(action.decode::<u32>().is_err());
    }

    #[test]
    fn an_action_over_the_limit_is_refused() {
        assert!(!ContactAction(vec![0; ContactAction::MAX_BYTES + 1]).is_valid());
        assert!(ContactAction(vec![0; ContactAction::MAX_BYTES]).is_valid());
    }
}
