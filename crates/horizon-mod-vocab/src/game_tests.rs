//! Functions registered for the in-world game test runner.
use crate::ServerHost;
use crate::wire::{Reader, Wire, WireError, Writer};
use std::{collections::BTreeMap, sync::Arc};

crate::wire::wire_handlers::handler!(dyn GameTestTicker, "GameTestTicker");

/// The outcome of a function invocation. Pending keeps the runner's timeout active.
#[derive(Clone, Default, horizon_mod_api_macros::Wire)]
pub enum GameTestResult {
    /// Complete this attempt successfully.
    Pass,
    /// Complete this attempt with an assertion message.
    Fail(String),
    /// Leave this attempt running until its timeout.
    #[default]
    Pending,
    /// Keep this attempt running and ask the ticker every tick, starting with the tick `run`
    /// returned on, until it answers [`GameTestTick::Pass`] or [`GameTestTick::Fail`] or the
    /// attempt times out. The runner keeps the ticker for this attempt only: a retry calls `run`
    /// again, so per-attempt state belongs in the ticker.
    Poll(Arc<dyn GameTestTicker>),
}
impl std::fmt::Debug for GameTestResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pass => f.write_str("Pass"),
            Self::Fail(message) => f.debug_tuple("Fail").field(message).finish(),
            Self::Pending => f.write_str("Pending"),
            Self::Poll(_) => f.write_str("Poll(..)"),
        }
    }
}
/// Two `Poll` results are equal when they hold the same ticker.
impl PartialEq for GameTestResult {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Pass, Self::Pass) | (Self::Pending, Self::Pending) => true,
            (Self::Fail(a), Self::Fail(b)) => a == b,
            (Self::Poll(a), Self::Poll(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}
impl Eq for GameTestResult {}

/// A polled attempt's answer for one tick.
#[derive(Clone, Debug, PartialEq, Eq, horizon_mod_api_macros::Wire)]
pub enum GameTestTick {
    /// Complete this attempt successfully on this tick.
    Pass,
    /// Complete this attempt with an assertion message on this tick.
    Fail(String),
    /// Not yet: ask again next tick. If the attempt times out, it fails with the reason the
    /// ticker gave on the timeout tick; an empty reason fails it as a timeout with no result.
    Waiting(String),
}
/// The answer of a ticker that couldn't answer, such as a wasm mod's that trapped: the attempt
/// fails, as it does when a native ticker panics.
impl Default for GameTestTick {
    fn default() -> Self {
        Self::Fail("the game test's ticker did not answer".into())
    }
}

/// The per-tick check of one attempt, returned by [`GameTestFunction::run`] in
/// [`GameTestResult::Poll`]: vanilla's `succeedWhen`-style sequences, built by the mod.
pub trait GameTestTicker: Send + Sync + 'static {
    /// Called on the tick `run` returned and then once every tick until the attempt ends, with
    /// the same level and structure as `run` and the current [`GameTestContext::tick`]. Keep
    /// the attempt's state in the ticker. A panic fails the attempt, as one in `run` does.
    fn tick(&self, host: &mut dyn ServerHost, context: &GameTestContext) -> GameTestTick;
}

/// API-owned coordinates for one attempt, valid only during its callback.
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub struct GameTestContext {
    /// The server level containing this attempt.
    pub level: crate::LevelRef,
    /// Structure origin in the current server level.
    pub origin: [i32; 3],
    /// Structure rotation in clockwise quarter turns.
    pub rotation: u8,
    /// The attempt's tick, vanilla's `GameTestHelper.getTick()`: 0 on the tick `run` is called,
    /// after the setup ticks, then counting up by one each tick.
    pub tick: i32,
}
impl GameTestContext {
    /// Construct the context at the engine boundary, on tick 0.
    pub fn new(level: crate::LevelRef, origin: [i32; 3], rotation: u8) -> Self {
        Self {
            level,
            origin,
            rotation,
            tick: 0,
        }
    }
    /// The same context on another tick.
    pub fn with_tick(mut self, tick: i32) -> Self {
        self.tick = tick;
        self
    }
    /// Convert structure-relative coordinates to world coordinates.
    pub fn absolute_pos(&self, [x, y, z]: [i32; 3]) -> [i32; 3] {
        let (x, z) = match self.rotation % 4 {
            1 => (-z, x),
            2 => (-x, -z),
            3 => (z, -x),
            _ => (x, z),
        };
        [self.origin[0] + x, self.origin[1] + y, self.origin[2] + z]
    }
}
/// A function run once at the start of each attempt, with the owning server level.
pub trait GameTestFunction: Send + Sync {
    /// Return an outcome; the runner owns retries, timeouts and completion.
    fn run(&self, host: &mut dyn ServerHost, context: &GameTestContext) -> GameTestResult;
}
/// Metadata for a function test added to the world's test-instance registry.
#[derive(Clone, Debug)]
#[non_exhaustive]
#[derive(horizon_mod_api_macros::Wire)]
pub struct GameTestDefinition {
    /// Structure template to place.
    pub structure: String,
    /// Registered function identifier.
    pub function: String,
    /// Environment definition selected by the runner.
    pub environment: String,
    /// Server dimension in which to run.
    pub dimension: String,
    /// Timeout in ticks.
    pub max_ticks: i32,
    /// Delay before invoking the function.
    pub setup_ticks: i32,
    /// Maximum attempts.
    pub max_attempts: i32,
    /// Number of successful attempts required.
    pub required_successes: i32,
    /// Whether failure counts as a required failure.
    pub required: bool,
}
impl GameTestDefinition {
    /// A required test with a 100-tick timeout, no setup delay and one attempt.
    pub fn new(structure: String, function: String) -> Self {
        Self {
            structure,
            function,
            environment: "minecraft:default".into(),
            dimension: "minecraft:overworld".into(),
            max_ticks: 100,
            setup_ticks: 0,
            max_attempts: 1,
            required_successes: 1,
            required: true,
        }
    }
}
/// Registration service supplied to the registration hook.
pub trait GameTests {
    /// Register a test instance; duplicate ids and invalid timing are refused.
    fn register_test(
        &mut self,
        id: &str,
        definition: GameTestDefinition,
    ) -> Result<(), &'static str>;
    /// Register a unique namespaced function. Vanilla identifiers are reserved.
    fn register(
        &mut self,
        id: &str,
        function: Arc<dyn GameTestFunction>,
    ) -> Result<(), &'static str>;
}
/// Load-owned function registry. Duplicate and invalid identifiers are refused.
#[derive(Default, Clone)]
pub struct GameTestFunctions {
    definitions: BTreeMap<String, GameTestDefinition>,
    functions: BTreeMap<String, Arc<dyn GameTestFunction>>,
}
impl GameTests for GameTestFunctions {
    fn register_test(
        &mut self,
        id: &str,
        definition: GameTestDefinition,
    ) -> Result<(), &'static str> {
        if crate::Identifier::parse(id).is_err()
            || !id.contains(':')
            || id.starts_with("minecraft:")
        {
            return Err("invalid or reserved test id");
        }
        if crate::Identifier::parse(&definition.function).is_err()
            || !definition.function.contains(':')
        {
            return Err("expected namespaced function id");
        }
        if self.definitions.contains_key(id) {
            return Err("duplicate test id");
        }
        if definition.max_ticks < 1
            || definition.setup_ticks < 0
            || definition.max_attempts < 1
            || definition.required_successes < 1
        {
            return Err("invalid test timing");
        }
        self.definitions.insert(id.into(), definition);
        Ok(())
    }

    /// Register a namespaced function. Vanilla identifiers are reserved.
    fn register(
        &mut self,
        id: &str,
        function: Arc<dyn GameTestFunction>,
    ) -> Result<(), &'static str> {
        let Some((namespace, path)) = id.split_once(':') else {
            return Err("expected namespaced function id");
        };
        if namespace == "minecraft"
            || namespace.is_empty()
            || path.is_empty()
            || !namespace
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"_.-".contains(&c))
            || !path
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"/_.-".contains(&c))
        {
            return Err("invalid or reserved function id");
        }
        if self.functions.contains_key(id) {
            return Err("duplicate function id");
        }
        self.functions.insert(id.into(), function);
        Ok(())
    }
}
impl GameTestFunctions {
    /// Test definitions collected at world load, in identifier order.
    pub fn definitions(&self) -> impl Iterator<Item = (&str, &GameTestDefinition)> {
        self.definitions
            .iter()
            .map(|(id, definition)| (id.as_str(), definition))
    }

    /// Resolve once when a test definition loads. The callback remains owned by that definition.
    pub fn resolve(&self, id: &str) -> Option<Arc<dyn GameTestFunction>> {
        self.functions.get(id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Pass;
    impl GameTestFunction for Pass {
        fn run(&self, _: &mut dyn ServerHost, _: &GameTestContext) -> GameTestResult {
            GameTestResult::Pass
        }
    }
    #[test]
    fn test_definitions_reject_duplicates_and_invalid_timing() {
        let mut registry = GameTestFunctions::default();
        let definition = GameTestDefinition::new("minecraft:empty".into(), "example:pass".into());
        registry
            .register_test("example:passing", definition.clone())
            .unwrap();
        assert!(
            registry
                .register_test("example:passing", definition.clone())
                .is_err()
        );
        let mut invalid = definition;
        invalid.max_ticks = 0;
        assert!(registry.register_test("example:invalid", invalid).is_err());
        let bare = GameTestDefinition::new("example:structure".into(), "pass".into());
        assert_eq!(
            registry.register_test("example:bare", bare),
            Err("expected namespaced function id")
        );
        assert_eq!(
            registry.definitions().map(|(id, _)| id).collect::<Vec<_>>(),
            ["example:passing"]
        );
    }
    #[test]
    fn resolved_functions_survive_registry_replacement_and_duplicates_preserve_original() {
        let mut registry = GameTestFunctions::default();
        let original: Arc<dyn GameTestFunction> = Arc::new(Pass);
        registry.register("example:pass", original.clone()).unwrap();
        assert!(registry.register("example:pass", Arc::new(Pass)).is_err());
        let resolved = registry.resolve("example:pass").unwrap();
        drop(registry);
        assert!(Arc::ptr_eq(&resolved, &original));
        for id in [
            "pass",
            "minecraft:always_pass",
            "Example:test",
            "example:",
            ":test",
            "example:a:b",
        ] {
            assert!(
                GameTestFunctions::default()
                    .register(id, Arc::new(Pass))
                    .is_err(),
                "{id}"
            );
        }
    }
}
