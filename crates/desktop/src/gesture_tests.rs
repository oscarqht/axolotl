use crate::app::SwipeState;
use std::time::{Duration, Instant};
#[test]
fn momentum_switches_once_until_the_gesture_is_idle() {
    let mut state = SwipeState::default();
    let now = Instant::now();
    assert_eq!(state.feed(30., 2., now), None);
    assert_eq!(
        state.feed(40., 2., now + Duration::from_millis(20)),
        Some(true)
    );
    for i in 2..20 {
        assert_eq!(
            state.feed(50., 0., now + Duration::from_millis(i * 20)),
            None
        );
    }
    assert_eq!(
        state.feed(-70., 0., now + Duration::from_millis(800)),
        Some(false)
    );
}
#[test]
fn vertical_and_small_input_does_not_switch_tabs() {
    let mut state = SwipeState::default();
    let now = Instant::now();
    assert_eq!(state.feed(10., 50., now), None);
    assert_eq!(state.feed(20., 0., now), None);
    assert_eq!(state.feed(-20., 0., now), None);
}
