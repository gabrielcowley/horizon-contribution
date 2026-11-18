//! Mod-owned world fluid registration values.
/// Simulation and bucket settings for a declared world fluid.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub struct WorldFluid {
    /// Ticks between flow steps, at least one.
    pub tick_delay: i32,
    /// Amount lost per block, from one to seven.
    pub drop_off: i32,
    /// Search distance for a way down, from one to four.
    pub slope_find_distance: i32,
    /// Whether two sources can create another source.
    pub source_conversion: bool,
    /// Whether a filled bucket is registered.
    pub bucket: bool,
    /// How entities feel it; none leaves it inert to them, as if it were air.
    pub immersion: Option<FluidImmersion>,
    /// Its block's `MapColor` id, 0 to 61; water's when none.
    pub map_color: Option<u8>,
}
impl Default for WorldFluid {
    fn default() -> Self {
        Self {
            tick_delay: 5,
            drop_off: 1,
            slope_find_distance: 4,
            source_conversion: false,
            bucket: true,
            immersion: None,
            map_color: None,
        }
    }
}

impl WorldFluid {
    /// Refuses values the simulation cannot run: no step, no spread, or an unbounded search.
    pub fn check(&self) -> Result<(), String> {
        if self.tick_delay < 1 {
            return Err("tick_delay is 1 or more".into());
        }
        if !(1..=7).contains(&self.drop_off) {
            return Err("drop_off is 1 to 7".into());
        }
        if !(1..=4).contains(&self.slope_find_distance) {
            return Err("slope_find_distance is 1 to 4".into());
        }
        if self.map_color.is_some_and(|c| c > 61) {
            return Err("map_color is a MapColor id, 0 to 61".into());
        }
        if let Some(immersion) = &self.immersion {
            immersion.check()?;
        }
        Ok(())
    }
}

/// How entities in a world fluid feel it, NeoForge's `FluidType.Properties` of the same names.
/// [`Default`] is NeoForge's defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub struct FluidImmersion {
    /// `canSwim`: entities move in it as in water, rise in it and swim in it.
    pub can_swim: bool,
    /// `canPushEntity`: its current pushes entities, at `motion_scale`.
    pub can_push_entity: bool,
    /// `motionScale`: how hard its current pushes, finite and 0 or more (water's 0.014).
    pub motion_scale: f64,
    /// `canDrown`: an entity's air runs out with its eyes in it.
    pub can_drown: bool,
    /// `canExtinguish`: it puts out a burning entity.
    pub can_extinguish: bool,
    /// `fallDistanceModifier`: what an entity's fall distance is multiplied by each tick in it,
    /// finite and 0 or more.
    pub fall_distance_modifier: f32,
    /// `supportsBoating`: a boat floats on it.
    pub supports_boating: bool,
}
impl Default for FluidImmersion {
    fn default() -> Self {
        Self {
            can_swim: true,
            can_push_entity: true,
            motion_scale: 0.014,
            can_drown: true,
            can_extinguish: false,
            fall_distance_modifier: 0.5,
            supports_boating: false,
        }
    }
}
impl FluidImmersion {
    /// Refuses a number no entity could be moved by: not finite, or negative. Never clamped.
    pub fn check(&self) -> Result<(), String> {
        if !self.motion_scale.is_finite() || self.motion_scale < 0.0 {
            return Err("immersion: motion_scale is a finite number, 0 or more".into());
        }
        if !self.fall_distance_modifier.is_finite() || self.fall_distance_modifier < 0.0 {
            return Err("immersion: fall_distance_modifier is a finite number, 0 or more".into());
        }
        Ok(())
    }
}
