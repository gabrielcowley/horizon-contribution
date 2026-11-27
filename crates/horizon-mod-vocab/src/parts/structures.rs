use super::*;
/// The structure types registered at startup, for the executable to hand to
/// the engine's `StructureRegistry`.
pub(crate) fn installed_structure_type_list()
-> &'static [crate::world::structure_type::StructureTypeInfo] {
    installed().map_or(&[], |installed| installed.structure_types.as_slice())
}

/// The feature types registered at startup, for the executable to hand to the engine's feature
/// decoder.
pub(crate) fn installed_feature_type_list() -> &'static [crate::world::feature_type::FeatureTypeInfo]
{
    installed().map_or(&[], |installed| installed.feature_types.as_slice())
}

/// The placement-modifier types registered at startup, for the executable to hand to the
/// engine's placement decoder.
pub(crate) fn installed_placement_modifier_type_list()
-> &'static [crate::world::placement_modifier_type::PlacementModifierTypeInfo] {
    installed().map_or(&[], |installed| {
        installed.placement_modifier_types.as_slice()
    })
}

impl dyn Registrar + '_ {
    /// Register structure type `name` (`tower` or `mymod:tower`), the way
    /// NeoForge's registries `STRUCTURE_TYPE` and `STRUCTURE_PIECE` register
    /// once at startup: a data pack structure with `"type": "modid:tower"`
    /// then generates by `ty`'s code — its own start and pieces, placed by the
    /// game's own spacing, biome and terrain-adaptation checks. The type's id
    /// is in a namespace owned by the calling mod's manifest (`mymod:tower`); another mod's
    /// namespace, an empty name and an id registered twice refuse the mods at
    /// startup, naming the mod. `ty`'s `load_piece` is its saved pieces'
    /// loader: keep the type for as long as the game runs.
    pub fn register_structure_type<T: crate::StructureType>(&mut self, name: &str, ty: T) {
        self.add_structure_type(name, Arc::new(ty));
    }
    /// Register feature type `name` (`vein` or `mymod:vein`), the way NeoForge's registry
    /// `FEATURE` registers once at startup: a data pack configured feature with
    /// `"type": "mymod:vein"` (26.3's inline shape, its settings beside `type`) is then decoded
    /// once by `ty` when the pack loads and placed by the feature it decoded wherever the game
    /// places features: decoration, nested placed features, growth and `/place feature`. The
    /// type's id is in a namespace owned by the calling mod's manifest; another mod's
    /// namespace, an empty name and an id registered twice refuse the mods at startup, naming
    /// the mod. Vanilla's own types keep their ids.
    pub fn register_feature_type<T: crate::FeatureType>(&mut self, name: &str, ty: T) {
        self.add_feature_type(name, Arc::new(ty));
    }
    /// Register placement-modifier type `name` (`filter` or `mymod:filter`), the way NeoForge's
    /// registry `PLACEMENT_MODIFIER_TYPE` registers once at startup: an entry of a data pack
    /// placed feature's `placement` list with `"type": "mymod:filter"` is then decoded once by
    /// `ty` when the pack loads, and runs by the modifier it decoded wherever the game places
    /// that placed feature: decoration, nested placed features and growth. The type's id is in
    /// a namespace owned by the calling mod's manifest; another mod's namespace, an empty name
    /// and an id registered twice refuse the mods at startup, naming the mod. Vanilla's own
    /// types keep their ids.
    pub fn register_placement_modifier_type<T: crate::PlacementModifierType>(
        &mut self,
        name: &str,
        ty: T,
    ) {
        self.add_placement_modifier_type(name, Arc::new(ty));
    }
}

/// Registrar operations for structures and worldgen features.
pub trait StructuresRegistrar {
    /// Register structure type `name` with `ty`; mods reach it through
    /// `register_structure_type`.
    ///
    /// On WebAssembly, worker calls run under the linked group's store lock and call budget.
    fn add_structure_type(&mut self, name: &str, ty: Arc<dyn crate::StructureType>);
    /// Register feature type `name` with `ty`; mods reach it through `register_feature_type`.
    ///
    /// On WebAssembly, worker calls run under the linked group's store lock and call budget.
    fn add_feature_type(&mut self, name: &str, ty: Arc<dyn crate::FeatureType>);
    /// Register placement-modifier type `name` with `ty`; mods reach it through
    /// `register_placement_modifier_type`.
    ///
    /// On WebAssembly, worker calls run under the linked group's store lock and call budget.
    fn add_placement_modifier_type(
        &mut self,
        name: &str,
        ty: Arc<dyn crate::PlacementModifierType>,
    );
}
