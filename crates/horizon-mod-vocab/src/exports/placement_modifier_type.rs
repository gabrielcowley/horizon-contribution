pub use world::placement_modifier_type::{
    PlacementModifier, PlacementModifierContext, PlacementModifierType,
};
mod __private {
    /// The placement-modifier types the mods registered, for the executable to hand to the
    /// engine's placement decoder before any world opens.
    pub use crate::world::placement_modifier_type::placement_modifier_type_declarations;
}
