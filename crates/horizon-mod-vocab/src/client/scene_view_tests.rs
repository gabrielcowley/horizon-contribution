//! `SceneView`'s projection and its inverse.
use super::SceneView;

const RECT: [f32; 4] = [10.0, 20.0, 110.0, 80.0];

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-3)
}

/// Unturned, the scene is seen from the north: east on the viewer's left, up up the
/// screen, north towards the viewer, a block `scale` pixels across.
#[test]
fn an_unturned_view_puts_the_center_mid_box_and_east_on_the_left() {
    let view = SceneView::new([0.5, 0.5, 0.5], 0.0, 0.0, 20.0);
    assert_eq!(view.to_gui(RECT, [0.5, 0.5, 0.5]), [60.0, 50.0, 0.0]);
    assert_eq!(view.to_gui(RECT, [1.5, 1.5, -0.5]), [40.0, 30.0, 20.0]);
    let (origin, direction) = view.ray_from_gui(RECT, [40.0, 30.0]);
    assert!(close(origin, [1.5, 1.5, 0.5]), "{origin:?}");
    assert!(
        close(direction, [0.0, 0.0, 1.0]),
        "south, away: {direction:?}"
    );
}

/// A ray through a GUI point meets the scene where `to_gui` draws it, and runs away
/// from the viewer, for any turn.
#[test]
fn ray_from_gui_inverts_to_gui() {
    for (yaw, pitch) in [(30.0, 25.0), (-135.0, 60.0), (210.0, -10.0), (180.0, 35.0)] {
        let view = SceneView::new([2.0, -1.0, 3.5], yaw, pitch, 16.0);
        let gui = [37.0, 64.5];
        let (origin, direction) = view.ray_from_gui(RECT, gui);
        assert!(close(view.to_gui(RECT, origin), [gui[0], gui[1], 0.0]));
        let length = direction.iter().map(|d| d * d).sum::<f32>().sqrt();
        assert!((length - 1.0).abs() < 1e-5, "unit");
        let further = std::array::from_fn(|i| origin[i] + 2.0 * direction[i]);
        assert!(
            close(view.to_gui(RECT, further), [gui[0], gui[1], -32.0]),
            "{yaw} {pitch}: two blocks further away, under the same GUI point"
        );
    }
}
