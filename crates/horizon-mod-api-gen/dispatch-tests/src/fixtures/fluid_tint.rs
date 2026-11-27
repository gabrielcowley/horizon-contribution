/// A tint part: records that it was asked, for which fluid, then gives its answer.
struct TintProbe(Option<u32>, Arc<Mutex<Vec<(String, Option<u32>)>>>);

impl FluidPart for TintProbe {
    fn fluid_subscribed(&self) -> &'static [Option<HookId>] {
        &[Some(HookId::FluidTint)]
    }
    fn fluid_tint(&self, event: &mut FluidTintEvent<'_>) -> Option<u32> {
        self.1
            .lock()
            .unwrap()
            .push((event.fluid.to_owned(), self.0));
        self.0
    }
}

#[test]
fn fluid_tint_takes_the_first_part_that_answers() {
    install(Vec::<ModEntry>::new().into());
    let asked = Arc::new(Mutex::new(Vec::new()));
    install_parts(Parts {
        fluids: [None, Some(0xFF8B_4513), Some(0xFF00_0000)]
            .map(|answer| leak(TintProbe(answer, asked.clone())) as &dyn FluidPart)
            .into(),
        ..Default::default()
    });

    let mut tint = None;
    dispatch_fluid_tint("brew:tea", &FluidStack, &mut tint);
    // `None` passes the stack on; the first answer wins and the parts after it aren't asked.
    assert_eq!(tint, Some(0xFF8B_4513));
    assert_eq!(
        *asked.lock().unwrap(),
        [
            ("brew:tea".to_owned(), None),
            ("brew:tea".to_owned(), Some(0xFF8B_4513))
        ]
    );

    // A tint already set (a draw's own colour) asks no part.
    asked.lock().unwrap().clear();
    let mut tint = Some(0xFFFF_FFFF);
    dispatch_fluid_tint("brew:tea", &FluidStack, &mut tint);
    assert_eq!(tint, Some(0xFFFF_FFFF));
    assert!(asked.lock().unwrap().is_empty());

    // With no parts, the tint is left to the fluid's look.
    install_parts(Parts::default());
    let mut tint = None;
    dispatch_fluid_tint("brew:tea", &FluidStack, &mut tint);
    assert_eq!(tint, None);
}
