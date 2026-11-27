//! Concrete authoring extracted verbatim from the vocabulary layer.
use super::*;

/// Fixtures that tests of other crates use to install Parts.
#[cfg(any(test, feature = "testing"))]
pub mod tests {
    #[cfg(test)]
    use super::*;
    #[cfg(test)]
    use crate::*;

    /// A structure type twice, one in an unowned namespace and one without
    /// a name: every registration is refused at startup, naming the mod.
    #[test]
    pub(crate) fn duplicate_and_foreign_structure_types_refuse_registration() {
        struct Tower;
        impl mod_api::StructureType for Tower {
            fn find_generation_point(
                &self,
                _: &mut dyn mod_api::GenerationContext,
            ) -> Option<mod_api::FoundStructure> {
                None
            }
            fn load_piece(
                &self,
                _: &dyn mod_api::BlockMetadata,
                _: &mod_api::Compound,
            ) -> Result<std::sync::Arc<dyn mod_api::StructurePiece>, mod_api::StructureError>
            {
                Err(mod_api::StructureError("fixture has no pieces".into()))
            }
        }
        struct Valid;
        #[mod_api::__subscriptions]
        impl Hooks for Valid {
            fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
                r.register_structure_type("tower", Tower);
            }
        }
        crate::parts::tests::install_parts_fixture(&[ModEntry::new("test", Box::new(Valid))])
            .unwrap();
        let types = crate::__private::structure_type_declarations();
        assert!(types.iter().any(|(id, _)| id == "test:tower"));
        assert_eq!(types.len(), 1);

        struct Invalid;
        #[mod_api::__subscriptions]
        impl Hooks for Invalid {
            fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
                r.register_structure_type("tower", Tower);
                r.register_structure_type("tower", Tower);
                r.register_structure_type("other:tower", Tower);
                r.register_structure_type("", Tower);
            }
        }
        let errors = install_parts(
            &[ModEntry::new("test", Box::new(Invalid))],
            &crate::parts::tests::rule_schema(),
            |_| false,
        )
        .unwrap_err();
        assert_eq!(errors.len(), 3);
        assert!(errors[0].contains("test registered structure type test:tower twice"));
        assert!(errors[1].contains("namespace"));
        assert!(errors[2].contains("invalid identifier \"test:\""));
    }

    /// A feature type in the mod's namespace is handed over; the same id twice and another
    /// mod's namespace refuse the mods at startup, naming the mod.
    #[test]
    pub(crate) fn duplicate_and_foreign_feature_types_refuse_registration() {
        struct Vein;
        impl mod_api::FeatureType for Vein {
            fn decode(
                &self,
                _: &mod_api::Tag,
                _: &mut dyn mod_api::FeatureDecode,
            ) -> Result<std::sync::Arc<dyn mod_api::ConfiguredFeature>, mod_api::FeatureError>
            {
                Err(mod_api::FeatureError("fixture has no features".into()))
            }
        }
        struct Valid;
        #[mod_api::__subscriptions]
        impl Hooks for Valid {
            fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
                r.register_feature_type("vein", Vein);
            }
        }
        crate::parts::tests::install_parts_fixture(&[ModEntry::new("test", Box::new(Valid))])
            .unwrap();
        let types = crate::__private::feature_type_declarations();
        assert_eq!(
            types.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(),
            ["test:vein"]
        );

        struct Invalid;
        #[mod_api::__subscriptions]
        impl Hooks for Invalid {
            fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
                r.register_feature_type("vein", Vein);
                r.register_feature_type("vein", Vein);
                r.register_feature_type("other:vein", Vein);
            }
        }
        let errors = install_parts(
            &[ModEntry::new("test", Box::new(Invalid))],
            &crate::parts::tests::rule_schema(),
            |_| false,
        )
        .unwrap_err();
        assert_eq!(errors.len(), 2);
        assert!(errors[0].contains("test registered feature type test:vein twice"));
        assert!(errors[1].contains("namespace"));
    }

    /// A placement-modifier type in the mod's namespace is handed over; the same id twice and
    /// another mod's namespace refuse the mods at startup, naming the mod.
    #[test]
    pub(crate) fn duplicate_and_foreign_placement_modifier_types_refuse_registration() {
        struct Filter;
        impl mod_api::PlacementModifierType for Filter {
            fn decode(
                &self,
                _: &mod_api::Tag,
                _: &mut dyn mod_api::FeatureDecode,
            ) -> Result<std::sync::Arc<dyn mod_api::PlacementModifier>, mod_api::FeatureError>
            {
                Err(mod_api::FeatureError("fixture has no modifiers".into()))
            }
        }
        struct Valid;
        #[mod_api::__subscriptions]
        impl Hooks for Valid {
            fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
                r.register_placement_modifier_type("filter", Filter);
            }
        }
        crate::parts::tests::install_parts_fixture(&[ModEntry::new("test", Box::new(Valid))])
            .unwrap();
        let types = crate::__private::placement_modifier_type_declarations();
        assert_eq!(
            types.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(),
            ["test:filter"]
        );

        struct Invalid;
        #[mod_api::__subscriptions]
        impl Hooks for Invalid {
            fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
                r.register_placement_modifier_type("filter", Filter);
                r.register_placement_modifier_type("filter", Filter);
                r.register_placement_modifier_type("other:filter", Filter);
            }
        }
        let errors = install_parts(
            &[ModEntry::new("test", Box::new(Invalid))],
            &crate::parts::tests::rule_schema(),
            |_| false,
        )
        .unwrap_err();
        assert_eq!(errors.len(), 2);
        assert!(errors[0].contains("test registered placement modifier type test:filter twice"));
        assert!(errors[1].contains("namespace"));
    }
}

impl StructuresRegistrar for PartRegistrar {
    fn add_structure_type(&mut self, name: &str, ty: Arc<dyn crate::StructureType>) {
        let id = match self.owned_declaration_id(name) {
            Ok(id) => id,
            Err(reason) => {
                self.refused.push(reason);
                return;
            }
        };
        let refusal = {
            self.structure_types
                .iter()
                .any(|t| *t.id == *id)
                .then(|| format!("{} registered structure type {id} twice", self.mod_id))
        };
        if let Some(refusal) = refusal {
            self.refused.push(refusal);
            return;
        }
        self.structure_types
            .push(StructureTypeInfo { id: id.into(), ty });
    }
    fn add_feature_type(&mut self, name: &str, ty: Arc<dyn crate::FeatureType>) {
        let id = match self.owned_declaration_id(name) {
            Ok(id) => id,
            Err(reason) => {
                self.refused.push(reason);
                return;
            }
        };
        if self.feature_types.iter().any(|t| *t.id == *id) {
            self.refused.push(format!(
                "{} registered feature type {id} twice",
                self.mod_id
            ));
            return;
        }
        self.feature_types
            .push(FeatureTypeInfo { id: id.into(), ty });
    }
    fn add_placement_modifier_type(
        &mut self,
        name: &str,
        ty: Arc<dyn crate::PlacementModifierType>,
    ) {
        let id = match self.owned_declaration_id(name) {
            Ok(id) => id,
            Err(reason) => {
                self.refused.push(reason);
                return;
            }
        };
        if self.placement_modifier_types.iter().any(|t| *t.id == *id) {
            self.refused.push(format!(
                "{} registered placement modifier type {id} twice",
                self.mod_id
            ));
            return;
        }
        self.placement_modifier_types
            .push(PlacementModifierTypeInfo { id: id.into(), ty });
    }
}
