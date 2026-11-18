//! Registration of in-world test functions.
pub trait Hooks {
    /// Register namespaced test functions before the world's test definitions load.
    /// Each function runs with a ServerHost once per attempt in the real test level.
    /// The runner retains ownership of retries, timeout and completion.
    #[notes = r#"## In-world game tests

`register_game_tests` runs once per world open before test-instance definitions resolve their functions.
Register an `Arc<dyn GameTestFunction>` through `event.functions.register("example:check", callback)`.
Add a test with `event.functions.register_test(id, GameTestDefinition::new(structure, function))`,
or use a data-pack `test_instance` with type `minecraft:function` and function `example:check` selects it.
The callback receives a `ServerHost` and `GameTestContext`: use `context.level` for level handles and
`context.absolute_pos(relative)` for rotated structure coordinates. Return `Pass`, `Fail(message)`,
`Pending` or `Poll(ticker)`; pending attempts keep the ordinary timeout. Each retry invokes the function again.

To check every tick, as vanilla's `succeedWhen` does, return `Poll(Arc<dyn GameTestTicker>)`. The runner
calls `ticker.tick(host, context)` on the same tick `run` returned and then once every tick, where vanilla
ticks a test's sequences, until it answers `GameTestTick::Pass` or `GameTestTick::Fail(message)`.
`Waiting(reason)` means not yet; if the attempt times out, it fails with the reason given on the timeout
tick, or with no result when the reason is empty. A pass on the timeout tick is too late, as in vanilla.
A panic in `tick` fails the attempt. A retry gets a fresh ticker from a fresh `run`, so keep the
attempt's state in the ticker. `context.tick` is vanilla's `getTick()`: 0 on the tick `run` is called,
after the setup ticks. Scheduled actions and sequences are built by the mod on this ticker.
Definitions retain resolved callbacks only for their world snapshot. Unknown functions fail normally;
`minecraft:always_pass` remains built in. Data-pack test definitions override code-registered defaults. Registration refuses duplicates, invalid names and the vanilla
namespace. There is no per-tick registry lookup."#]
    #[hook(
        side = "server",
        contract = "listen",
        host = "read",
        neoforge = "RegisterGameTestsEvent"
    )]
    fn register_game_tests();
}
