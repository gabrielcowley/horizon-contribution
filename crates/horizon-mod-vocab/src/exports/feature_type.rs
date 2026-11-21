pub use world::feature_type::{
    ConfiguredFeature, FeatureContext, FeatureDecode, FeatureError, FeatureType, RuleTestHandle,
    TargetBlockState,
};
mod __private {
    /// The feature types the mods registered, for the executable to hand to the engine's
    /// feature decoder before any world opens.
    pub use crate::world::feature_type::{
        feature_type_declarations, rule_test_handle, rule_test_index, target_block_state,
    };
}
