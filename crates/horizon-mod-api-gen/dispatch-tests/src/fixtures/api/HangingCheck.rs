#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HangingCheck {
    pub unobstructed: bool,
    pub supported: bool,
    pub alone: bool,
}
