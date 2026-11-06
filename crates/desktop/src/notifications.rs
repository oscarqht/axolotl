use std::collections::HashMap;
use workspace_core::{TabId, Workspace};

impl crate::app::BrowserApp {
    pub(crate) fn clear_tab_notifications(&mut self, tab: TabId) {
        clear_for_origin(&self.model, &mut self.notifications, tab);
    }
}

fn tuple_origin(value: &str) -> Option<url::Origin> {
    match url::Url::parse(value).ok()?.origin() {
        origin @ url::Origin::Tuple(..) => Some(origin),
        url::Origin::Opaque(_) => None,
    }
}

/// Use current model URLs, including hidden spaces, floating panes and pins.
/// The focused tab is always cleared, even if its URL has no comparable origin.
fn clear_for_origin(model: &Workspace, counts: &mut HashMap<TabId, u32>, tab: TabId) {
    counts.remove(&tab);
    if counts.is_empty() {
        return;
    }
    let Some(origin) = model.tab(tab).and_then(|tab| tuple_origin(&tab.url)) else {
        return;
    };
    counts.retain(|id, _| {
        model
            .tab(*id)
            .and_then(|tab| tuple_origin(&tab.url))
            .is_none_or(|candidate| candidate != origin)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use workspace_core::{Axis, Rect};

    #[test]
    fn notification_origins_normalize_scheme_host_and_effective_port() {
        let origin = tuple_origin("https://EXAMPLE.com/path?q=1#fragment").unwrap();
        for url in [
            "https://example.com:443/other",
            "https://user:pass@example.com/",
        ] {
            assert_eq!(tuple_origin(url), Some(origin.clone()));
        }
        for url in [
            "http://example.com",
            "https://example.com:444",
            "https://sub.example.com",
        ] {
            assert_ne!(tuple_origin(url), Some(origin.clone()));
        }
        assert_eq!(
            tuple_origin("http://example.com:80"),
            tuple_origin("http://example.com")
        );
        assert_eq!(
            tuple_origin("https://[::1]:443/a"),
            tuple_origin("https://[::1]/b")
        );
        for url in [
            "not a URL",
            "about:blank",
            "data:text/plain,hello",
            "file:///tmp/a",
        ] {
            assert_eq!(tuple_origin(url), None);
        }
    }

    #[test]
    fn notification_clearing_spans_spaces_floating_panes_and_pins_without_source_badge() {
        let mut model = Workspace::new("https://example.com/viewed");
        let focused = model.focused_pane().active_tab;
        let pin = model.add_tab("https://example.com:443/pin");
        assert!(model.pin_tab(model.focused_pane().id, pin));
        let floating = model.split_focused(Axis::Horizontal, "https://example.com/floating");
        let floated = model.pane(floating).unwrap().active_tab;
        assert!(model.float_pane(
            floating,
            Rect {
                x: 0.,
                y: 0.,
                width: 400.,
                height: 300.
            },
            None
        ));
        model.add_space("Hidden", "https://EXAMPLE.com/hidden");
        let hidden = model.focused_pane().active_tab;
        let unrelated = model.add_tab("https://example.com:444/unrelated");
        let mut counts = HashMap::from([(pin, 2), (floated, 3), (hidden, 4), (unrelated, 5)]);
        clear_for_origin(&model, &mut counts, focused);
        assert_eq!(counts, HashMap::from([(unrelated, 5)]));

        counts.insert(focused, 1);
        counts.insert(pin, 2);
        // Navigation changes the matching set immediately.
        model.tab_mut(unrelated).unwrap().url = "https://example.com/now-matching".into();
        clear_for_origin(&model, &mut counts, focused);
        assert!(counts.is_empty());
    }

    #[test]
    fn notification_opaque_and_invalid_urls_only_clear_the_focused_tab() {
        for url in [
            "about:blank",
            "data:text/plain,hello",
            "file:///tmp/a",
            "invalid",
        ] {
            let mut model = Workspace::new(url);
            let first = model.focused_pane().active_tab;
            let second = model.add_tab(url);
            let mut counts = HashMap::from([(first, 2), (second, 3)]);
            clear_for_origin(&model, &mut counts, first);
            assert_eq!(counts, HashMap::from([(second, 3)]));
        }
    }
}
