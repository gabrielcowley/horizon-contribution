//! Fluid interactions a mod declares: what a fluid becomes when it touches another fluid or a
//! block, as NeoForge's `FluidInteractionRegistry.InteractionInformation`.

/// What a fluid turns into beside another fluid or a block (NeoForge's
/// `FluidInteractionRegistry.addInteraction`), declared as data: `LiquidBlock.shouldSpreadLiquid`
/// tries each converting fluid's interactions for each neighbour above and beside it (never
/// below), vanilla's lava rules first, then the mods' in load and declaration order. The first
/// that matches places its block through `fluid_place_block`, fires level event 1501 and stops the
/// fluid spreading. Declare it with `Registrar::declare_fluid_interaction` or in
/// `data/<mod id>/horizon/fluid_interaction/<path>.json`, a list of them.
///
/// Ids are `namespace:path`. A block state is written as `/setblock` takes it,
/// `minecraft:basalt[axis=x]`, the properties left out at their default.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub struct FluidInteraction {
    /// The fluid that converts: `minecraft:lava`, `minecraft:water` or a world fluid, named in
    /// either form. Not a tank-only fluid.
    pub fluid: String,
    /// What it must touch.
    pub when: FluidInteractionWhen,
    /// What it becomes.
    pub becomes: FluidInteractionBecomes,
}

/// The condition of a [`FluidInteraction`]: exactly one of `touching_fluid` and
/// `touching_block`, and optionally `block_below`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub struct FluidInteractionWhen {
    /// The neighbour holds this fluid, source or flowing (NeoForge matches its `FluidType`).
    /// Vanilla's own water rule reads back as `#minecraft:water`, the live tag; a declaration
    /// can't name a tag.
    pub touching_fluid: Option<String>,
    /// The neighbour is this block, in any state.
    pub touching_block: Option<String>,
    /// The block under the converting fluid is this one, as vanilla's basalt needs soul soil.
    pub block_below: Option<String>,
}

/// The block states a [`FluidInteraction`] places, chosen by the converting fluid's own state.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub struct FluidInteractionBecomes {
    /// When the converting fluid is a source: lava's obsidian.
    pub source: String,
    /// When it is flowing: lava's cobblestone.
    pub flowing: String,
}

impl FluidInteraction {
    /// An interaction of `fluid` touching the fluid `touching`, becoming `source` or `flowing`.
    pub fn touching_fluid(fluid: &str, touching: &str, source: &str, flowing: &str) -> Self {
        Self::new(fluid, Some(touching), None, source, flowing)
    }
    /// An interaction of `fluid` touching the block `touching`, becoming `source` or `flowing`.
    pub fn touching_block(fluid: &str, touching: &str, source: &str, flowing: &str) -> Self {
        Self::new(fluid, None, Some(touching), source, flowing)
    }
    /// The same interaction, also needing `block` under the converting fluid.
    pub fn with_block_below(mut self, block: &str) -> Self {
        self.when.block_below = Some(block.into());
        self
    }
    fn new(
        fluid: &str,
        touching_fluid: Option<&str>,
        touching_block: Option<&str>,
        source: &str,
        flowing: &str,
    ) -> Self {
        Self {
            fluid: fluid.into(),
            when: FluidInteractionWhen {
                touching_fluid: touching_fluid.map(Into::into),
                touching_block: touching_block.map(Into::into),
                block_below: None,
            },
            becomes: FluidInteractionBecomes {
                source: source.into(),
                flowing: flowing.into(),
            },
        }
    }
    /// Refuses a declaration whose shape is wrong, before anything is looked up: a condition
    /// missing or doubled, a result missing, an id that is not `namespace:path`. Ids that name
    /// nothing are refused when the registry loads.
    pub fn check(&self) -> Result<(), String> {
        let when = &self.when;
        match (&when.touching_fluid, &when.touching_block) {
            (None, None) => return Err("when: names no condition".into()),
            (Some(_), Some(_)) => {
                return Err("when: names both touching_fluid and touching_block".into());
            }
            _ => {}
        }
        let id = |field: &str, value: &str| {
            if namespaced(value) {
                Ok(())
            } else {
                Err(format!("{field}: {value:?} is not a namespace:path id"))
            }
        };
        id("fluid", &self.fluid)?;
        for (field, value) in [
            ("when.touching_fluid", &when.touching_fluid),
            ("when.touching_block", &when.touching_block),
            ("when.block_below", &when.block_below),
        ] {
            if let Some(value) = value {
                id(field, value)?;
            }
        }
        for (field, state) in [
            ("becomes.source", &self.becomes.source),
            ("becomes.flowing", &self.becomes.flowing),
        ] {
            if state.is_empty() {
                return Err(format!("{field}: is missing"));
            }
            id(field, state.split('[').next().unwrap_or(""))?;
        }
        Ok(())
    }
}

fn namespaced(id: &str) -> bool {
    id.split_once(':').is_some_and(|(namespace, path)| {
        !namespace.is_empty()
            && !path.is_empty()
            && namespace
                .bytes()
                .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.'))
            && path
                .bytes()
                .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.' | b'/'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shapes_the_engine_cannot_read_are_refused() {
        let ok = FluidInteraction::touching_fluid(
            "minecraft:lava",
            "create:honey",
            "minecraft:obsidian",
            "create:limestone",
        );
        assert_eq!(ok.check(), Ok(()));
        let mut both = ok.clone();
        both.when.touching_block = Some("minecraft:stone".into());
        assert_eq!(
            both.check(),
            Err("when: names both touching_fluid and touching_block".into())
        );
        let mut none = ok.clone();
        none.when.touching_fluid = None;
        assert_eq!(none.check(), Err("when: names no condition".into()));
        let mut missing = ok.clone();
        missing.becomes.flowing.clear();
        assert_eq!(missing.check(), Err("becomes.flowing: is missing".into()));
        let bare = FluidInteraction::touching_block("lava", "minecraft:stone", "a:b", "a:b");
        assert_eq!(
            bare.check(),
            Err("fluid: \"lava\" is not a namespace:path id".into())
        );
    }
}
