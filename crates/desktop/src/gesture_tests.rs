use crate::app::SwipeState;
use std::time::{Duration, Instant};
#[test]
fn momentum_fires_once_until_the_gesture_is_idle() {
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
fn vertical_and_small_input_does_not_trigger_a_swipe() {
    let mut state = SwipeState::default();
    let now = Instant::now();
    assert_eq!(state.feed(10., 50., now), None);
    assert_eq!(state.feed(20., 0., now), None);
    assert_eq!(state.feed(-20., 0., now), None);
}

#[test]
fn wheel_threshold_and_directions_are_preserved() {
    for (x, forward) in [(65_f64, true), (-65_f64, false)] {
        let mut state = SwipeState::default();
        let now = Instant::now();
        assert_eq!(state.feed(x.signum() * 64., 0., now), None);
        assert_eq!(state.feed(x.signum(), 0., now), Some(forward));
        assert_eq!(state.feed(-x * 2., 0., now), None);
    }
}

#[test]
fn page_history_commands_are_authenticated_separately_from_tab_switching() {
    use crate::browser::authenticated_command;
    for command in [
        "history-back",
        "history-forward",
        "swipe-next",
        "swipe-prev",
    ] {
        assert_eq!(
            authenticated_command(&format!("private:{command}"), "private"),
            Some(command)
        );
        assert_eq!(
            authenticated_command(&format!("wrong:{command}"), "private"),
            None
        );
    }
}
