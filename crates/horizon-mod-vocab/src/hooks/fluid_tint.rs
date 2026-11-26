//! The `fluid_tint` hook: a declaration the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// The `0xAARRGGBB` colour a stack of fluid `fluid` is drawn in, read each time a tank
    /// (`BlockEntityDrawContext::fluid_box`), a stream (`BlockEntityDrawContext::fluid_stream`)
    /// or a GUI slot (`DrawContext::fluid_stack`) draws it: NeoForge's
    /// `IClientFluidTypeExtensions.getTintColor(FluidStack)`. Answer from `stack` alone, its
    /// components included (a potion's colour from its `minecraft:potion_contents`), or `None`
    /// to leave it to the next part and then to the fluid's look. The first part to answer wins,
    /// in dispatch order. It runs on the client with no host, once per drawn stack and with no
    /// cache: keep it a pure, fast function of the stack. A draw's own `argb` still wins over it.
    #[notes = r#"## Tinting a fluid by its stack

- **`fluid_tint(event)`** on a `FluidPart`: the colour one stack of the fluid is drawn in, in a tank, a stream and a
  GUI slot alike, NeoForge's `getTintColor(FluidStack)`. Attach the part to any fluid a mod declared, one only a tank
  holds (`declare_fluid`) as well as a world fluid. Without an answer the stack takes its look's tint (`tint` in
  `data/<mod>/horizon/fluid/<name>.json`), made opaque; water draws in its item colour and lava in white."#]
    #[hook(
        side = "client",
        contract = "replace",
        out = "tint",
        scope = "fluid",
        key = "fluid",
        target = "horizon_client_render::fluid_models::native_fluid_tint",
        neoforge = "IClientFluidTypeExtensions.getTintColor(FluidStack)"
    )]
    fn fluid_tint();
}
