# Feature types

Register a `FeatureType` in `Hooks::register` to place a configured feature by your own code. A
data pack configured feature with `"type": "mymod:vein"` then uses it wherever the game places
features: biome decoration, nested placed features (random selectors and the like), growth and
`/place feature`:

```rust,ignore
fn register(&self, registrar: &mut dyn Registrar, _: &RegistrationContext) {
    registrar.register_feature_type("vein", VeinType);
}
```

Use an ID in your mod's namespace. A duplicate ID, an ID in another mod's namespace, or an empty
name stops startup with a message naming your mod. Vanilla's types keep their IDs.

## Decoding

`FeatureType::decode(&self, config: &Tag, decode: &mut dyn FeatureDecode)` is the type's codec. In
26.3 a configured feature holds its settings inline, beside `type`:

```json
{ "type": "mymod:vein", "size": 32, "targets": [ ... ] }
```

`config` is that object without `type`, converted to NBT once, when the data pack loads and again
on `/reload`. Return an `Arc<dyn ConfiguredFeature>`. An `Err(FeatureError(..))` refuses the
feature at load, naming the type and your message, as a malformed built-in feature is refused:
nothing fails later, when a chunk first reaches it.

`FeatureDecode` decodes the vanilla values a configuration embeds, against the blocks of the world
being loaded:

| Decoder | Vanilla codec |
|---|---|
| `block_state` | `BlockState.CODEC`: a block id, or `{"id": ..., "properties": {...}}` |
| `rule_test` | `RuleTest.CODEC`: returns a `RuleTestHandle` kept with the feature |
| `target_block_states` | An ore's `targets` list (`OreConfiguration.TargetBlockState`): each a rule and a state |

## Placing

`ConfiguredFeature::place(&self, context: &mut dyn FeatureContext)` is vanilla's `Feature.place`:
`Ok(true)` when something was placed (`/place feature` fails on `false`), `Ok(false)` when nothing
was.

| `FeatureContext` access | Vanilla |
|---|---|
| `origin`, `min_y`, `max_y` | The origin and the level's build height (`max_y` inclusive) |
| `random` | The caller's own `RandomSource`, as a `StructureRandom`: decoration's seeded random, the nested feature's caller's, the growing block's, or the level's for `/place feature` |
| `height` | `WorldGenLevel.getHeight` |
| `can_write` | `WorldGenLevel.ensureCanWrite` |
| `block` | `BulkSectionAccess.getBlockState`; `None` outside the world's sections, never a chunk load |
| `set_section_block` | A `BulkSectionAccess` section write, as vanilla's `ore` makes: no updates, heightmaps or block entities |
| `set_block` | `WorldGenLevel.setBlock(pos, state, flags)` |
| `is_air`, `metadata` | `BlockState.isAir`, and block metadata to read properties |
| `rule_test` | `RuleTest.test` of a rule this feature decoded, drawing from the same random |

A write outside the writable region or the build height answers `false`. The modifiers of the
placed feature around yours, the decoration step and its seed stay the game's: your draws
interleave with theirs exactly as a built-in feature's do.

Your writes are kept only when the call succeeds, in the order you made them, and `block` reads
your own earlier writes back. A call that returns `Err`, panics (native) or traps (wasm) keeps
none of its writes; the game logs the type and the origin, and switches the type off for the
session, so every later call fails explicitly instead of leaving terrain out. A call may make at
most 65,536 block reads and writes; one more fails it the same way, without switching the type
off. A `next_int_bound` bound below 1, which vanilla refuses with an exception, answers 0 and fails
the call after it returns.

Decoration runs on the worldgen worker threads with no `Host`: keep a type and its features
stateless and deterministic. These traits also work in `mod.wasm`; its calls run under the
dependency group's store lock and call budget, as structure types' do.

Entities, block entities, scheduled ticks and nested placement from a mod's feature are not
available yet, and custom placement modifiers are not registrable.
