use super::*;

impl Game<'_> {
    /// `Entity.lerpMotion` for this client's copy of a general entity row of type
    /// `kind` with ID `entity`: the server sent `received`, and the copy had
    /// `current`. A part's answer, in `motion`, is the velocity the copy keeps; with
    /// none, it keeps `received`. Client-side.
    pub(in crate::game) fn native_general_motion_received(
        &self,
        kind: &str,
        entity: mod_api::EntityRef,
        current: [f64; 3],
        received: [f64; 3],
        motion: &mut Option<[f64; 3]>,
    ) {
        self.fire_general_motion_received(kind, entity, current, received, motion);
    }
}
