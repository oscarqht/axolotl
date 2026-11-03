use super::*;

fn assert_invariants(workspace: &Workspace) {
    assert!(!workspace.spaces.is_empty());
    assert!(workspace
        .spaces
        .iter()
        .any(|s| s.id == workspace.active_space));
    let mut all_ids = std::collections::HashSet::new();
    for space in &workspace.spaces {
        assert!(all_ids.insert(space.id));
        assert!(!space.panes.is_empty());
        assert!(space.pane(space.focused_pane).is_some());
        let mut layout_ids = space.layout.pane_ids();
        let mut pane_ids: Vec<_> = space.panes.iter().map(|p| p.id).collect();
        layout_ids.sort_unstable();
        pane_ids.sort_unstable();
        assert_eq!(layout_ids, pane_ids);
        for pane in &space.panes {
            assert!(all_ids.insert(pane.id));
            assert!(!pane.tabs.is_empty());
            assert!(pane.tabs.iter().any(|t| t.id == pane.active_tab));
            for tab in &pane.tabs {
                assert!(all_ids.insert(tab.id));
            }
        }
    }
}

#[test]
fn nested_splits_close_and_space_switch_keep_focus_valid() {
    let mut workspace = Workspace::new("https://example.com/");
    let home = workspace.active_space;
    let original = workspace.focused_pane().id;
    let right = workspace.split_focused(Axis::Horizontal, "https://rust-lang.org/");
    let bottom = workspace.split_focused(Axis::Vertical, "https://gpui.rs/");
    assert_eq!(
        workspace.active_space().layout.pane_ids(),
        [original, right, bottom]
    );
    assert_eq!(workspace.focused_pane().id, bottom);
    assert!(!workspace.focus_pane(u64::MAX));
    assert!(workspace.close_pane(right));
    assert_eq!(
        workspace.active_space().layout.pane_ids(),
        [original, bottom]
    );
    assert!(workspace.close_pane(bottom));
    assert_eq!(workspace.focused_pane().id, original);
    assert!(!workspace.close_pane(original));
    let second = workspace.add_space("Research", "about:blank");
    assert_eq!(workspace.active_space, second);
    assert!(!workspace.focus_pane(original));
    assert!(workspace.switch_space(home));
    assert!(!workspace.switch_space(u64::MAX));
    assert!(workspace.close_space(home));
    assert_eq!(workspace.active_space, second);
    assert!(!workspace.close_space(second));
    assert_invariants(&workspace);
}

#[test]
fn tab_stacks_retain_identity_and_replace_last_closed_tab() {
    let mut workspace = Workspace::default();
    let pane = workspace.focused_pane().id;
    let blank = workspace.focused_pane().active_tab;
    let first = workspace.add_tab("https://example.com/");
    let second = workspace.add_tab("https://rust-lang.org/");
    assert_eq!(workspace.focused_pane().active_tab, second);
    assert!(workspace.switch_tab(pane, first));
    assert!(workspace.close_tab(pane, second));
    assert_eq!(workspace.focused_pane().active_tab, first);
    assert!(workspace.close_tab(pane, first));
    assert_eq!(workspace.focused_pane().active_tab, blank);
    assert!(workspace.close_tab(pane, blank));
    assert_ne!(workspace.focused_pane().active_tab, blank);
    assert_eq!(workspace.focused_pane().active_tab().url, "about:blank");
    assert!(!workspace.close_tab(pane, blank));
    assert!(!workspace.switch_tab(pane, blank));
    assert!(workspace.tab(first).is_none());
    let tab = workspace.focused_pane().active_tab;
    workspace.tab_mut(tab).unwrap().title = "Blank".into();
    assert_eq!(workspace.tab(tab).unwrap().title, "Blank");
    assert_invariants(&workspace);
}

#[test]
fn grid_preserves_all_tabs_even_when_consolidating_extra_panes() {
    let mut workspace = Workspace::default();
    let mut tabs = vec![workspace.focused_pane().active_tab];
    for _ in 0..5 {
        workspace.split_focused(Axis::Horizontal, "https://example.com/");
        tabs.push(workspace.focused_pane().active_tab);
        tabs.push(workspace.add_tab("https://rust-lang.org/"));
    }
    let focused_tab = workspace.focused_pane().active_tab;
    workspace.grid("about:blank");
    assert_eq!(workspace.active_space().panes.len(), 4);
    for tab in tabs {
        assert!(workspace.tab(tab).is_some());
    }
    assert_eq!(workspace.focused_pane().active_tab, focused_tab);
    assert_invariants(&workspace);
    let mut fresh = Workspace::default();
    fresh.grid("https://example.com/");
    assert_eq!(fresh.active_space().panes.len(), 4);
    assert_invariants(&fresh);
}

#[test]
fn geometry_partitions_nested_splits_and_reports_drag_containers() {
    let mut workspace = Workspace::default();
    workspace.grid("about:blank");
    let geometry =
        workspace
            .active_space()
            .layout
            .geometry(Rect::new(20.0, 30.0, 1006.0, 606.0), 6.0, 80.0);
    assert_eq!(geometry.panes.len(), 4);
    assert_eq!(geometry.dividers.len(), 3);
    assert_eq!(
        geometry.panes[0].bounds,
        Rect::new(20.0, 30.0, 500.0, 300.0)
    );
    assert_eq!(
        geometry.panes[1].bounds,
        Rect::new(20.0, 336.0, 500.0, 300.0)
    );
    assert_eq!(
        geometry.panes[2].bounds,
        Rect::new(526.0, 30.0, 500.0, 300.0)
    );
    assert_eq!(
        geometry.panes[3].bounds,
        Rect::new(526.0, 336.0, 500.0, 300.0)
    );
    let root = geometry.dividers[0];
    assert_eq!(root.container, Rect::new(20.0, 30.0, 1006.0, 606.0));
    assert!(workspace.resize_split(root.id, 0.8));
    assert!(!workspace.resize_split(root.id, f32::NAN));
    assert!(!workspace.resize_split(u64::MAX, 0.5));
    let resized = workspace
        .active_space()
        .layout
        .geometry(root.container, 6.0, 80.0);
    assert_eq!(resized.panes[0].bounds.width, 800.0);
    assert_eq!(resized.panes[2].bounds.width, 200.0);
}

#[test]
fn undersized_and_invalid_bounds_never_produce_negative_or_nan_sizes() {
    let mut workspace = Workspace::default();
    workspace.grid("about:blank");
    for size in [0.0, 1.0, 5.0, 10.0, 100.0, f32::NAN, -20.0] {
        let geometry =
            workspace
                .active_space()
                .layout
                .geometry(Rect::new(0.0, 0.0, size, size), 6.0, 120.0);
        for pane in geometry.panes {
            assert!(pane.bounds.width.is_finite() && pane.bounds.width >= 0.0);
            assert!(pane.bounds.height.is_finite() && pane.bounds.height >= 0.0);
        }
    }
}

#[test]
fn navigation_normalizes_web_addresses_and_encodes_search_queries() {
    assert_eq!(
        normalize_url("  example.com  ").unwrap(),
        "https://example.com/"
    );
    assert_eq!(
        normalize_url("localhost:3300/path").unwrap(),
        "http://localhost:3300/path"
    );
    assert_eq!(normalize_url("[::1]:3300/").unwrap(), "http://[::1]:3300/");
    assert_eq!(
        normalize_url("example.com:8080/path").unwrap(),
        "https://example.com:8080/path"
    );
    assert_eq!(
        normalize_url("//example.com").unwrap(),
        "https://example.com/"
    );
    assert_eq!(normalize_url("about:blank").unwrap(), "about:blank");
    let url = url::Url::parse(&normalize_url("rust & gpui layout").unwrap()).unwrap();
    assert_eq!(url.query_pairs().next().unwrap().1, "rust & gpui layout");
    for unsafe_url in [
        "javascript:alert(1)",
        "file:///etc/passwd",
        "data:text/html,test",
        "about:config",
        "https://user:password@example.com/",
        "https://",
        "",
        "https://exam\nple.com",
    ] {
        assert!(normalize_url(unsafe_url).is_err(), "accepted {unsafe_url}");
    }
}

#[test]
fn long_command_sequence_keeps_layout_tabs_and_focus_consistent() {
    let mut workspace = Workspace::default();
    let mut seed = 0x45ac_2211_u64;
    for step in 0..1500 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        match (seed >> 32) % 12 {
            0 if workspace.spaces.len() < 5 => {
                workspace.add_space(format!("Space {step}"), "about:blank");
            }
            1 => {
                let index = (seed as usize) % workspace.spaces.len();
                workspace.switch_space(workspace.spaces[index].id);
            }
            2 => {
                workspace.close_space(workspace.active_space);
            }
            3 | 4 if workspace.active_space().panes.len() < 10 => {
                let axis = if step % 2 == 0 {
                    Axis::Horizontal
                } else {
                    Axis::Vertical
                };
                workspace.split_focused(axis, "about:blank");
            }
            5 => {
                workspace.close_pane(workspace.focused_pane().id);
            }
            6 => {
                workspace.add_tab("https://example.com/");
            }
            7 => {
                workspace.close_tab(
                    workspace.focused_pane().id,
                    workspace.focused_pane().active_tab,
                );
            }
            8 => workspace.grid("about:blank"),
            9 => {
                let space = workspace.active_space();
                let index = (seed as usize) % space.panes.len();
                workspace.focus_pane(space.panes[index].id);
            }
            10 => {
                let pane = workspace.focused_pane();
                let index = (seed as usize) % pane.tabs.len();
                workspace.switch_tab(pane.id, pane.tabs[index].id);
            }
            _ => {
                let geometry = workspace.active_space().layout.geometry(
                    Rect::new(0.0, 0.0, 20.0, 20.0),
                    6.0,
                    100.0,
                );
                if let Some(divider) = geometry.dividers.first() {
                    workspace.resize_split(divider.id, (seed % 100) as f32 / 100.0);
                }
            }
        }
        assert_invariants(&workspace);
        let extent = (seed % 1200) as f32;
        let geometry = workspace.active_space().layout.geometry(
            Rect::new(0.0, 0.0, extent, extent),
            6.0,
            100.0,
        );
        for pane in geometry.panes {
            assert!(pane.bounds.x >= 0.0 && pane.bounds.y >= 0.0);
            assert!(pane.bounds.width >= 0.0 && pane.bounds.height >= 0.0);
            assert!(pane.bounds.x + pane.bounds.width <= extent + 0.001);
            assert!(pane.bounds.y + pane.bounds.height <= extent + 0.001);
        }
    }
}
