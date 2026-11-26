use super::{FluidFile, FluidImmersion, FluidLook, WorldFluid};
use mod_vocab::wire::{Reader, Wire, WireError, Writer};

fn round<T: Wire + PartialEq + std::fmt::Debug>(value: T) {
    let mut out = Writer::new();
    out.put(&value);
    assert_eq!(out.error(), None);
    assert_eq!(Reader::decode::<T>(out.bytes()).unwrap(), value);
    for end in 0..out.bytes().len() {
        assert!(Reader::decode::<T>(&out.bytes()[..end]).is_err());
    }
}

#[test]
fn fluid_wire_round_trips_and_truncation() {
    round(WorldFluid {
        tick_delay: 12,
        drop_off: 2,
        slope_find_distance: 3,
        source_conversion: true,
        bucket: false,
        immersion: Some(FluidImmersion {
            can_swim: true,
            can_push_entity: true,
            motion_scale: 0.007,
            can_drown: false,
            can_extinguish: false,
            fall_distance_modifier: 0.25,
            supports_boating: true,
        }),
        map_color: Some(27),
    });
    round(FluidLook {
        id: "a:oil".into(),
        sprite: Some("a:block/oil".into()),
        flowing: Some("a:block/oil_flow".into()),
        tint: Some(0x3F2A10),
        light_level: 7,
        lighter_than_air: true,
        world: Some(WorldFluid {
            tick_delay: 9,
            drop_off: 3,
            slope_find_distance: 2,
            source_conversion: false,
            bucket: true,
            immersion: None,
            map_color: None,
        }),
    });
    round(FluidFile {
        sprite: Some("a:block/brine".into()),
        flowing: Some("a:block/brine_flow".into()),
        tint: Some(0x123456),
        light_level: Some(15),
        lighter_than_air: None,
        world: None,
    });
}

#[test]
fn fluid_wire_invalid_option_tag() {
    assert_eq!(Reader::decode::<FluidFile>(&[2]), Err(WireError::Tag(2)));
    // Empty id followed by an invalid sprite Option tag.
    assert_eq!(
        Reader::decode::<FluidLook>(&[0, 0, 0, 0, 2]),
        Err(WireError::Tag(2))
    );
}

/// Out-of-range entity rules and map colours are refused at declaration, never clamped.
#[test]
fn world_fluid_check_refuses_entity_rules_out_of_range() {
    let with = |immersion: FluidImmersion| WorldFluid {
        immersion: Some(immersion),
        ..WorldFluid::default()
    };
    assert!(with(FluidImmersion::default()).check().is_ok());
    for bad in [
        FluidImmersion {
            motion_scale: -0.001,
            ..FluidImmersion::default()
        },
        FluidImmersion {
            motion_scale: f64::NAN,
            ..FluidImmersion::default()
        },
        FluidImmersion {
            fall_distance_modifier: f32::INFINITY,
            ..FluidImmersion::default()
        },
        FluidImmersion {
            fall_distance_modifier: -1.0,
            ..FluidImmersion::default()
        },
    ] {
        assert!(with(bad).check().is_err(), "{bad:?}");
    }
    let colour = |map_color| WorldFluid {
        map_color: Some(map_color),
        ..WorldFluid::default()
    };
    assert!(colour(61).check().is_ok());
    assert!(colour(62).check().is_err());
}
