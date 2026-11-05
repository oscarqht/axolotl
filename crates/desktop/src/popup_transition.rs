//! Animate native surfaces together; CSS cannot fade a sibling WKWebView.
use crate::native_webview::NativeWebView;
use std::time::Duration;

pub(crate) fn duration(opening: bool) -> Duration {
    if reduced_motion() {
        Duration::ZERO
    } else {
        Duration::from_millis(if opening { 180 } else { 140 })
    }
}

fn reduced_motion() -> bool {
    #[cfg(target_os = "macos")]
    {
        #[cfg(feature = "smoke")]
        if let Some(value) = TEST_REDUCED_MOTION.with(|value| value.get()) {
            return value;
        }
        objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
    }
    #[cfg(not(target_os = "macos"))]
    true
}

#[cfg(all(feature = "smoke", target_os = "macos"))]
thread_local! {
    static TEST_REDUCED_MOTION: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

#[cfg(all(feature = "smoke", target_os = "macos"))]
pub(crate) fn set_test_reduced_motion(value: Option<bool>) {
    TEST_REDUCED_MOTION.with(|state| state.set(value));
}

#[cfg(target_os = "macos")]
const KEY: &str = "axolotl.link-popup.opacity";

pub(crate) fn animate(
    view: &NativeWebView,
    opening: bool,
    initial: bool,
    duration: Duration,
) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSNumber, NSString};
        use objc2_quartz_core::{
            CABasicAnimation, CACurrentMediaTime, CAMediaTiming, CAMediaTimingFunction,
        };
        use wry::WebViewExtMacOS;
        let native = view.webview();
        native.setWantsLayer(true);
        let layer = native
            .layer()
            .ok_or_else(|| anyhow::anyhow!("Popup layer unavailable"))?;
        let from = if initial {
            0.
        } else {
            // Reversing an unfinished fade continues at the displayed opacity.
            unsafe { layer.presentationLayer() }
                .map_or(native.alphaValue(), |p| f64::from(p.opacity()))
        };
        let to = if opening { 1. } else { 0. };
        let key = NSString::from_str(KEY);
        layer.removeAnimationForKey(&key);
        native.setAlphaValue(to);
        if !duration.is_zero() {
            let animation =
                CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("opacity")));
            unsafe {
                animation.setFromValue(Some(&NSNumber::new_f64(from)));
                animation.setToValue(Some(&NSNumber::new_f64(to)));
            }
            animation.setBeginTime(CACurrentMediaTime());
            animation.setDuration(duration.as_secs_f64());
            let timing = if opening {
                CAMediaTimingFunction::functionWithControlPoints(0.16, 1., 0.3, 1.)
            } else {
                CAMediaTimingFunction::functionWithControlPoints(0.4, 0., 1., 1.)
            };
            animation.setTimingFunction(Some(&timing));
            layer.addAnimation_forKey(&animation, Some(&key));
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (view, opening, initial, duration);
    Ok(())
}

pub(crate) fn reset(view: &NativeWebView) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::NSString;
        use wry::WebViewExtMacOS;
        let native = view.webview();
        if let Some(layer) = native.layer() {
            layer.removeAnimationForKey(&NSString::from_str(KEY));
        }
        // Promotion transfers the live view; it must never retain a closing alpha.
        native.setAlphaValue(1.);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = view;
    Ok(())
}

#[cfg(all(feature = "smoke", target_os = "macos"))]
pub(crate) fn state(view: &NativeWebView) -> serde_json::Value {
    use objc2_foundation::NSString;
    use wry::WebViewExtMacOS;
    let native = view.webview();
    let layer = native.layer();
    serde_json::json!({
        "model": native.alphaValue(),
        "presentation": layer.as_ref().and_then(|l| unsafe { l.presentationLayer() }).map(|p| p.opacity()),
        "animating": layer.as_ref().is_some_and(|l| unsafe { l.animationForKey(&NSString::from_str(KEY)).is_some() }),
    })
}
