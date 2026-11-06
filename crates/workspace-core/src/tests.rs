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
fn closing_last_tab_collapses_nested_panes_and_transfers_focus() {
    let mut w = Workspace::default();
    let original = w.focused_pane().id;
    let right = w.split_focused(Axis::Horizontal, "about:blank");
    let bottom = w.split_focused(Axis::Vertical, "about:blank");
    let closed = w.focused_pane().active_tab;
    assert!(w.close_tab(bottom, closed));
    assert!(w.pane(bottom).is_none());
    assert!(w.tab(closed).is_none());
    assert_eq!(w.active_space().layout.pane_ids(), [original, right]);
    assert_eq!(w.focused_pane().id, original);
    let geometry = w
        .active_space()
        .layout
        .geometry(Rect::new(0., 0., 1000., 800.), 6., 100.);
    assert_eq!(geometry.dividers.len(), 1);
    assert_eq!(
        geometry
            .panes
            .iter()
            .find(|p| p.id == right)
            .unwrap()
            .bounds
            .height,
        800.
    );
    // Closing an unfocused pane must preserve the current focus.
    let right_tab = w.pane(right).unwrap().active_tab;
    assert!(w.close_tab(right, right_tab));
    assert_eq!(w.active_space().layout, Layout::Pane(original));
    assert_eq!(w.focused_pane().id, original);
    assert_invariants(&w);
}

#[test]
fn closing_tabs_selects_neighbor_and_preserves_nonempty_pane() {
    let mut w = Workspace::default();
    let pane = w.split_focused(Axis::Horizontal, "about:blank");
    let first = w.focused_pane().active_tab;
    let middle = w.add_tab("https://example.com/middle");
    let last = w.add_tab("https://example.com/last");
    w.switch_tab(pane, middle);
    assert!(w.close_tab(pane, middle));
    assert_eq!(w.focused_pane().active_tab, last);
    assert!(w.close_tab(pane, first));
    assert_eq!(w.focused_pane().active_tab, last);
    assert_eq!(w.active_space().panes.len(), 2);
    assert_invariants(&w);
}

#[test]
fn closing_last_space_tab_uses_close_space_selection_and_preserves_survivors() {
    for closed_index in 0..3 {
        let mut w = Workspace::new("https://example.com/first");
        w.add_tab("https://example.com/first-active");
        w.add_space("Middle", "https://example.com/middle");
        w.add_space("Last", "https://example.com/last");
        w.split_focused(Axis::Horizontal, "https://example.com/panel");
        let closing = w.spaces[closed_index].id;
        w.switch_space(closing);
        // Retain multiple tabs and panels in survivors, but only one tab in the closing space.
        let pane = w.focused_pane().id;
        let others: Vec<_> = w
            .active_space()
            .panes
            .iter()
            .filter(|p| p.id != pane)
            .map(|p| p.id)
            .collect();
        for other in others {
            assert!(w.close_pane(other));
        }
        while w.focused_pane().tabs.len() > 1 {
            assert!(w.close_tab(pane, w.focused_pane().active_tab));
        }
        let tab = w.focused_pane().active_tab;
        let mut expected = w.clone();
        assert!(expected.close_space(closing));
        assert!(w.close_tab(pane, tab));
        assert_eq!(w, expected);
        assert_invariants(&w);
    }
}

#[test]
fn closing_panel_tab_keeps_space_while_another_panel_has_tabs() {
    let mut w = Workspace::new("https://example.com/survivor");
    let survivor = w.spaces[0].clone();
    let space = w.add_space("Split", "https://example.com/main");
    let original = w.focused_pane().id;
    let panel = w.split_focused(Axis::Horizontal, "https://example.com/panel");
    let tab = w.focused_pane().active_tab;
    assert!(w.close_tab(panel, tab));
    assert_eq!(w.spaces.len(), 2);
    assert_eq!(w.active_space, space);
    assert_eq!(w.focused_pane().id, original);
    assert_eq!(w.active_space().layout, Layout::Pane(original));
    assert_eq!(w.spaces[0], survivor);
    assert_invariants(&w);
}

#[test]
fn closing_final_pane_tab_closes_space_and_invalid_closes_are_atomic() {
    let mut w = Workspace::default();
    let home = w.active_space;
    let home_pane = w.focused_pane().id;
    let home_tab = w.focused_pane().active_tab;
    let other = w.add_space("Other", "https://example.com");
    let pane = w.focused_pane().id;
    let tab = w.focused_pane().active_tab;
    let before = w.spaces.clone();
    for (pane, tab) in [
        (home_pane, home_tab),
        (pane, home_tab),
        (u64::MAX, tab),
        (pane, u64::MAX),
    ] {
        assert!(!w.close_tab(pane, tab));
        assert_eq!(w.spaces, before);
    }
    assert!(w.close_tab(pane, tab));
    assert_eq!(w.active_space, home);
    assert_eq!(w.spaces.len(), 1);
    assert!(w.spaces.iter().all(|s| s.id != other));
    assert!(w.pane(pane).is_none());
    assert!(w.tab(tab).is_none());
    assert_eq!(w.spaces[0], before[0]);
    let before = w.clone();
    assert!(!w.close_tab(pane, tab));
    assert_eq!(w, before);
    assert_invariants(&w);
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

#[test]
fn docking_inserts_activates_and_preserves_tab_identity() {
    for index in 0..=3 {
        let mut w = Workspace::default();
        let source = w.focused_pane().id;
        let moved = w.add_tab("https://example.com/state");
        w.tab_mut(moved).unwrap().title = "Retained title".into();
        let original = w.tab(moved).unwrap().clone();
        let destination = w.split_focused(Axis::Horizontal, "about:blank");
        w.add_tab("https://rust-lang.org");
        w.add_tab("https://gpui.rs");
        let mut expected = w
            .focused_pane()
            .tabs
            .iter()
            .map(|t| t.id)
            .collect::<Vec<_>>();
        expected.insert(index, moved);
        assert!(w.move_tab(source, moved, destination, index));
        assert_eq!(w.focused_pane().id, destination);
        assert_eq!(w.focused_pane().active_tab, moved);
        assert_eq!(w.tab(moved), Some(&original));
        assert_eq!(
            w.focused_pane()
                .tabs
                .iter()
                .map(|t| t.id)
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(w.pane(source).unwrap().tabs.len(), 1);
        assert_invariants(&w);
    }
}

#[test]
fn docking_last_tab_collapses_nested_source_and_expands_sibling() {
    let mut w = Workspace::default();
    let destination = w.focused_pane().id;
    let sibling = w.split_focused(Axis::Horizontal, "about:blank");
    let source = w.split_focused(Axis::Vertical, "https://example.com");
    let moved = w.focused_pane().active_tab;
    assert!(w.move_tab(source, moved, destination, 0));
    assert!(w.pane(source).is_none());
    let g = w
        .active_space()
        .layout
        .geometry(Rect::new(0., 0., 1000., 800.), 6., 100.);
    assert_eq!(g.dividers.len(), 1);
    assert_eq!(
        g.panes
            .iter()
            .find(|p| p.id == sibling)
            .unwrap()
            .bounds
            .height,
        800.
    );
    assert_eq!(w.focused_pane().active_tab, moved);
    assert_invariants(&w);
}

#[test]
fn invalid_docking_is_atomic_and_cannot_cross_spaces() {
    let mut w = Workspace::default();
    let source = w.focused_pane().id;
    let moved = w.focused_pane().active_tab;
    let destination = w.split_focused(Axis::Horizontal, "about:blank");
    let before = w.active_space().clone();
    for (src, tab, dst, index) in [
        (source, moved, destination, 2),
        (source, u64::MAX, destination, 0),
        (u64::MAX, moved, destination, 0),
        (source, moved, u64::MAX, 0),
    ] {
        assert!(!w.move_tab(src, tab, dst, index));
        assert_eq!(w.active_space(), &before);
    }
    w.add_space("Other", "about:blank");
    let before = w.spaces.clone();
    assert!(!w.move_tab(source, moved, w.focused_pane().id, 0));
    assert_eq!(w.spaces, before);
    assert_invariants(&w);
}

#[test]
fn edge_targets_use_content_thirds_and_normalized_corner_distance() {
    let r = Rect::new(20., 80., 900., 300.);
    for (x, y, expected) in [
        (0.1, 0.5, Some(SplitEdge::Left)),
        (0.9, 0.5, Some(SplitEdge::Right)),
        (0.5, 0.1, Some(SplitEdge::Top)),
        (0.5, 0.9, Some(SplitEdge::Bottom)),
        (0.1, 0.2, Some(SplitEdge::Left)),
        (0.2, 0.1, Some(SplitEdge::Top)),
        (0., 0., Some(SplitEdge::Top)),
        (0.25, 0.25, Some(SplitEdge::Top)),
        (0.75, 0.25, Some(SplitEdge::Top)),
        (0.25, 0.75, Some(SplitEdge::Bottom)),
        (0.75, 0.75, Some(SplitEdge::Bottom)),
        (0.5, 0.5, None),
        (-0.01, 0.5, None),
        (1., 0.5, None),
        (0.5, -0.01, None),
    ] {
        assert_eq!(
            SplitEdge::at(r, r.x + x * r.width, r.y + y * r.height),
            expected
        );
    }
    assert_eq!(SplitEdge::at(r, f32::NAN, 100.), None);
    assert_eq!(SplitEdge::at(Rect::default(), 0., 0.), None);
    assert_eq!(
        SplitEdge::Right.preview(r),
        Rect::new(470., 80., 450., 300.)
    );
    assert_eq!(
        SplitEdge::Bottom.preview(r),
        Rect::new(20., 230., 900., 150.)
    );
}

#[test]
fn edge_splits_retain_tabs_focus_order_and_collapse_nested_sources() {
    for edge in [
        SplitEdge::Left,
        SplitEdge::Right,
        SplitEdge::Top,
        SplitEdge::Bottom,
    ] {
        let mut w = Workspace::default();
        let source = w.focused_pane().id;
        let tab = w.focused_pane().active_tab;
        let before = w.clone();
        assert_eq!(w.split_tab(source, tab, source, edge), None);
        assert_eq!(format!("{w:?}"), format!("{before:?}"));
        w.add_tab("https://example.com/retained");
        w.tab_mut(tab).unwrap().title = "Retained title".into();
        let original = w.tab(tab).unwrap().clone();
        let new = w.split_tab(source, tab, source, edge).unwrap();
        assert_eq!(w.tab(tab), Some(&original));
        assert_eq!(w.focused_pane().id, new);
        assert_eq!(w.focused_pane().active_tab, tab);
        let ids = w.active_space().layout.pane_ids();
        assert_eq!(
            ids,
            if edge.is_first() {
                vec![new, source]
            } else {
                vec![source, new]
            }
        );
        let destination = w.split_focused(Axis::Vertical, "https://example.com/target");
        let moved = w.split_tab(new, tab, destination, edge).unwrap();
        assert!(w.pane(new).is_none());
        assert_eq!(w.focused_pane().id, moved);
        assert_eq!(w.tab(tab), Some(&original));
        assert_eq!(w.active_space().panes.len(), 3);
        assert_invariants(&w);
        let before = w.clone();
        assert_eq!(w.split_tab(source, tab, destination, edge), None);
        assert_eq!(w.split_tab(moved, tab, u64::MAX, edge), None);
        assert_eq!(format!("{w:?}"), format!("{before:?}"));
        w.add_space("Other", "about:blank");
        let before = w.clone();
        assert_eq!(w.split_tab(moved, tab, destination, edge), None);
        assert_eq!(format!("{w:?}"), format!("{before:?}"));
    }
}

#[test]
fn reordering_uses_original_insertion_slots_and_retains_identity() {
    let mut w = Workspace::default();
    let pane = w.focused_pane().id;
    let a = w.focused_pane().active_tab;
    let b = w.add_tab("https://b.test");
    let c = w.add_tab("https://c.test");
    assert!(w.move_tab(pane, a, pane, 3));
    assert_eq!(
        w.focused_pane()
            .tabs
            .iter()
            .map(|t| t.id)
            .collect::<Vec<_>>(),
        [b, c, a]
    );
    assert!(w.move_tab(pane, a, pane, 0));
    assert_eq!(
        w.focused_pane()
            .tabs
            .iter()
            .map(|t| t.id)
            .collect::<Vec<_>>(),
        [a, b, c]
    );
    assert_eq!(w.focused_pane().active_tab, a);
    let before = w.active_space().clone();
    assert!(!w.move_tab(pane, a, pane, 4));
    assert_eq!(w.active_space(), &before);
    assert_invariants(&w);
}

#[test]
fn whole_panel_merge_preserves_order_active_tab_and_other_panels() {
    let mut w = Workspace::default();
    let source = w.focused_pane().id;
    let a = w.focused_pane().active_tab;
    let b = w.add_tab("https://b.test");
    let destination = w.split_focused(Axis::Horizontal, "https://destination.test");
    let existing = w.focused_pane().active_tab;
    let other = w.split_focused(Axis::Vertical, "https://other.test");
    assert!(w.move_pane(source, destination, None));
    assert!(w.pane(source).is_none());
    assert_eq!(w.focused_pane().id, destination);
    assert_eq!(
        w.focused_pane()
            .tabs
            .iter()
            .map(|t| t.id)
            .collect::<Vec<_>>(),
        [existing, a, b]
    );
    assert_eq!(w.focused_pane().active_tab, b);
    assert!(w.pane(other).is_some());
    assert_invariants(&w);
}

#[test]
fn whole_panel_can_move_to_every_edge_without_replacing_the_stack() {
    for edge in [
        SplitEdge::Left,
        SplitEdge::Right,
        SplitEdge::Top,
        SplitEdge::Bottom,
    ] {
        let mut w = Workspace::default();
        let source = w.focused_pane().id;
        w.add_tab("https://kept.test");
        let stack = w.focused_pane().clone();
        let destination = w.split_focused(Axis::Horizontal, "about:blank");
        assert!(w.move_pane(source, destination, Some(edge)));
        assert_eq!(w.pane(source), Some(&stack));
        assert_eq!(w.focused_pane().id, source);
        let ids = w.active_space().layout.pane_ids();
        assert_eq!(
            ids,
            if edge.is_first() {
                vec![source, destination]
            } else {
                vec![destination, source]
            }
        );
        let before = w.active_space().clone();
        assert!(!w.move_pane(source, source, Some(edge)));
        assert!(!w.move_pane(source, u64::MAX, Some(edge)));
        assert_eq!(w.active_space(), &before);
        assert_invariants(&w);
    }
}

#[test]
fn swiping_wraps_in_both_directions_and_handles_one_tab() {
    let mut w = Workspace::default();
    let pane = w.focused_pane().id;
    let first = w.focused_pane().active_tab;
    assert!(w.cycle_tab(pane, false));
    assert_eq!(w.focused_pane().active_tab, first);
    let last = w.add_tab("https://last.test");
    w.cycle_tab(pane, true);
    assert_eq!(w.focused_pane().active_tab, first);
    w.cycle_tab(pane, false);
    assert_eq!(w.focused_pane().active_tab, last);
    assert_invariants(&w);
}

#[test]
fn linked_tabs_are_inserted_after_the_source_in_its_own_pane() {
    let mut w = Workspace::new("https://example.com/source");
    let pane = w.focused_pane().id;
    let source = w.focused_pane().active_tab;
    let following = w.add_tab("https://example.com/following");
    let other = w.split_focused(Axis::Horizontal, "about:blank");
    let other_tabs = w.pane(other).unwrap().clone();
    let source_page = w.tab(source).unwrap().clone();
    let sibling = w.add_tab_after(source, "https://example.com/link").unwrap();
    assert_eq!(w.focused_pane().id, pane);
    assert_eq!(w.focused_pane().active_tab, sibling);
    assert_eq!(
        w.pane(pane)
            .unwrap()
            .tabs
            .iter()
            .map(|t| t.id)
            .collect::<Vec<_>>(),
        [source, sibling, following]
    );
    assert_eq!(w.tab(source), Some(&source_page));
    assert_eq!(w.pane(other), Some(&other_tabs));
    assert_invariants(&w);
}

#[test]
fn linked_tab_events_ignore_closed_sources_and_inactive_spaces() {
    let mut w = Workspace::default();
    let source = w.focused_pane().active_tab;
    let home = w.active_space;
    w.add_space("Other", "about:blank");
    assert_eq!(w.add_tab_after(source, "https://example.com"), None);
    assert_eq!(w.focused_pane().tabs.len(), 1);
    w.switch_space(home);
    let last = w.add_tab_after(source, "https://example.com").unwrap();
    let sibling = w.add_tab_after(last, "https://example.org").unwrap();
    assert_eq!(w.focused_pane().tabs.last().unwrap().id, sibling);
    let pane = w.focused_pane().id;
    w.close_tab(pane, source);
    let before = w.focused_pane().clone();
    assert_eq!(w.add_tab_after(source, "https://example.com"), None);
    assert_eq!(w.focused_pane(), &before);
    assert_invariants(&w);
}

#[test]
fn custom_title_overrides_page_title_until_cleared() {
    let mut w = Workspace::default();
    let tab = w.focused_pane().active_tab;
    w.tab_mut(tab).unwrap().title = "Page".into();
    assert_eq!(w.tab(tab).unwrap().display_title(), "Page");

    assert!(w.set_custom_title(tab, "  Pinned  "));
    assert_eq!(w.tab(tab).unwrap().display_title(), "Pinned");

    // Page title changes keep updating underneath but are not displayed.
    w.tab_mut(tab).unwrap().title = "Changed".into();
    assert_eq!(w.tab(tab).unwrap().display_title(), "Pinned");

    // Blank input clears the override and reveals the latest page title.
    assert!(w.set_custom_title(tab, "   "));
    assert_eq!(w.tab(tab).unwrap().custom_title, None);
    assert_eq!(w.tab(tab).unwrap().display_title(), "Changed");
    assert!(!w.set_custom_title(9999, "Nope"));
}

#[test]
fn custom_title_survives_moves_between_panes() {
    let mut w = Workspace::default();
    let source = w.focused_pane().id;
    let tab = w.add_tab("about:blank");
    assert!(w.set_custom_title(tab, "Keep"));
    let destination = w.split_focused(Axis::Horizontal, "about:blank");
    assert!(w.move_tab(source, tab, destination, 0));
    assert_eq!(w.tab(tab).unwrap().display_title(), "Keep");
}

#[test]
fn space_title_is_independent_of_pages_focus_and_selection_and_round_trips() {
    let mut workspace = Workspace::default();
    let first = workspace.active_space;
    let tab = workspace.focused_pane().active_tab;
    workspace.tab_mut(tab).unwrap().title = "Page one".into();
    let second = workspace.add_space("Other", "about:blank");
    assert!(workspace.set_space_title(first, "  Work  "));
    assert_eq!(workspace.active_space, second);
    workspace.switch_space(first);
    assert_eq!(workspace.active_space().display_title(), "Work");
    assert_eq!(workspace.tab(tab).unwrap().display_title(), "Page one");
    workspace.add_tab("https://example.com");
    workspace
        .tab_mut(workspace.focused_pane().active_tab)
        .unwrap()
        .title = "Page two".into();
    assert_eq!(workspace.active_space().display_title(), "Work");
    let restored = Workspace::from_json(&workspace.to_json()).unwrap();
    assert_eq!(restored.active_space().display_title(), "Work");
    assert!(workspace.set_space_title(first, " "));
    assert_eq!(workspace.active_space().display_title(), "Page two");
    let mut old: serde_json::Value = serde_json::from_str(&restored.to_json()).unwrap();
    for space in old["spaces"].as_array_mut().unwrap() {
        space.as_object_mut().unwrap().remove("custom_title");
    }
    assert!(Workspace::from_json(&old.to_string())
        .unwrap()
        .active_space()
        .custom_title
        .is_none());
    assert!(!workspace.set_space_title(u64::MAX, "Missing"));
}

#[test]
fn space_details_save_together_and_survive_session_restore() {
    let mut workspace = Workspace::default();
    let first = workspace.active_space;
    let tab = workspace.focused_pane().active_tab;
    workspace.tab_mut(tab).unwrap().title = "Page title".into();
    let other = workspace.add_space("Other", "about:blank");
    assert!(workspace.spaces.iter().all(|space| space.icon.is_none()));
    assert!(!workspace.to_json().contains("\"icon\""));

    assert!(workspace.set_space_details(first, "  Work  ", Some("  🧑‍💻  ")));
    assert_eq!(workspace.active_space, other);
    assert_eq!(workspace.tab(tab).unwrap().title, "Page title");
    assert!(workspace.tab(tab).unwrap().custom_title.is_none());
    assert!(workspace.active_space().icon.is_none());
    let restored = Workspace::from_json(&workspace.to_json()).unwrap();
    let space = restored
        .spaces
        .iter()
        .find(|space| space.id == first)
        .unwrap();
    assert_eq!(space.custom_title.as_deref(), Some("Work"));
    assert_eq!(space.icon.as_deref(), Some("🧑‍💻"));
    assert_eq!(restored.active_space, other);

    // Title-only callers retain the chosen icon.
    assert!(workspace.set_space_title(first, "Renamed"));
    assert_eq!(workspace.spaces[0].icon.as_deref(), Some("🧑‍💻"));
    assert!(workspace.set_space_details(first, " ", None));
    assert_eq!(workspace.spaces[0].custom_title, None);
    assert_eq!(workspace.spaces[0].icon, None);
    assert_eq!(workspace.spaces[0].display_title(), "Page title");
    assert!(workspace.set_space_details(first, "", Some(" ")));
    assert_eq!(workspace.spaces[0].icon, None);
    assert!(!workspace.set_space_details(u64::MAX, "Missing", Some("🚀")));

    // Existing sessions have no icon property and must continue to load.
    let mut legacy: serde_json::Value = serde_json::from_str(&restored.to_json()).unwrap();
    for space in legacy["spaces"].as_array_mut().unwrap() {
        space.as_object_mut().unwrap().remove("icon");
    }
    let legacy = Workspace::from_json(&legacy.to_string()).unwrap();
    assert!(legacy.spaces.iter().all(|space| space.icon.is_none()));
    assert_eq!(legacy.spaces[0].custom_title.as_deref(), Some("Work"));
}

#[test]
fn search_providers_encode_queries_without_changing_url_navigation() {
    use crate::{normalize_url_with_engine, SearchEngine};
    assert_eq!(
        normalize_url("hello world").unwrap(),
        "https://www.google.com/search?q=hello+world"
    );
    for (engine, endpoint) in [
        (SearchEngine::Google, "https://www.google.com/search"),
        (SearchEngine::DuckDuckGo, "https://duckduckgo.com/"),
        (SearchEngine::Bing, "https://www.bing.com/search"),
    ] {
        assert_eq!(SearchEngine::from_id(engine.id()), Some(engine));
        let query = "café & rust + #中文?";
        let url = url::Url::parse(&normalize_url_with_engine(query, engine).unwrap()).unwrap();
        assert_eq!(url.as_str().split('?').next().unwrap(), endpoint);
        assert_eq!(
            url.query_pairs().collect::<Vec<_>>(),
            vec![("q".into(), query.into())]
        );
        for address in [
            "https://example.org/a?q=hello#part",
            "example.com",
            "localhost:3300/path",
            "//example.org/a",
            "about:blank",
        ] {
            assert_eq!(
                normalize_url_with_engine(address, engine),
                normalize_url(address)
            );
        }
        for address in [
            "",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "https://user:pass@example.org",
            "hello\nworld",
        ] {
            assert!(normalize_url_with_engine(address, engine).is_err());
        }
    }
    assert_eq!(SearchEngine::from_id("custom"), None);
}

#[test]
fn space_reordering_preserves_selection_contents_and_snapshot_order() {
    let mut workspace = Workspace::new("about:blank");
    let a = workspace.active_space;
    let b = workspace.add_space("B", "https://example.com/");
    let c = workspace.add_space("C", "about:blank");
    let original = workspace.spaces.clone();
    let ids = |w: &Workspace| w.spaces.iter().map(|s| s.id).collect::<Vec<_>>();
    assert!(workspace.move_space(a, 3));
    assert_eq!(ids(&workspace), vec![b, c, a]);
    assert!(workspace.move_space(a, 0));
    assert_eq!(workspace.spaces, original);
    assert!(!workspace.move_space(b, 1));
    assert!(!workspace.move_space(b, 2));
    assert!(!workspace.move_space(u64::MAX, 0));
    assert!(workspace.move_space(c, 0));
    assert_eq!(ids(&workspace), vec![c, a, b]);
    assert_eq!(workspace.active_space, c);
    let restored = Workspace::from_json(&workspace.to_json()).unwrap();
    assert_eq!(restored.spaces, workspace.spaces);
    assert_eq!(restored.active_space, c);
}

#[test]
fn pins_keep_identity_and_unpin_into_the_current_space_and_focused_pane() {
    let mut w = Workspace::new("https://example.com/");
    let home = w.active_space;
    let source = w.focused_pane().id;
    let id = w.focused_pane().active_tab;
    w.set_custom_title(id, "Mail");
    assert!(w.pin_tab(source, id));
    assert_eq!(w.pinned_tabs[0].tab.id, id);
    assert_eq!(w.tab(id).unwrap().display_title(), "Mail");
    assert_ne!(w.focused_pane().active_tab, id);
    assert_eq!(w.focused_pane().tabs.len(), 1);
    assert!(!w.pin_tab(source, id));
    let second = w.add_space("Work", "about:blank");
    let target = w.split_focused(Axis::Horizontal, "about:blank");
    assert!(w.unpin_tab(id));
    assert_eq!(w.active_space, second);
    assert_eq!(w.focused_pane().id, target);
    assert_eq!(w.focused_pane().active_tab, id);
    assert_eq!(w.tab(id).unwrap().custom_title.as_deref(), Some("Mail"));
    assert!(w.pinned_tabs.is_empty());
    w.switch_space(home);
    assert!(!w.focused_pane().tabs.iter().any(|t| t.id == id));
}

#[test]
fn pinning_last_tab_collapses_a_split_and_preserves_other_tabs() {
    let mut w = Workspace::default();
    let first = w.focused_pane().id;
    let second = w.split_focused(Axis::Horizontal, "https://example.com/");
    let tab = w.focused_pane().active_tab;
    assert!(w.pin_tab(second, tab));
    assert_eq!(w.focused_pane().id, first);
    assert_eq!(w.active_space().layout.pane_ids(), vec![first]);
    assert_eq!(w.pinned_tabs[0].tab.id, tab);
    let tab2 = w.add_tab("https://docs.rs/");
    assert!(w.pin_tab(first, tab2));
    assert_eq!(w.focused_pane().tabs.len(), 1);
    assert_eq!(w.pinned_tabs.len(), 2);
}

#[test]
fn pin_order_and_sizes_round_trip_and_old_sessions_remain_compatible() {
    let mut w = Workspace::default();
    let pane = w.focused_pane().id;
    let first = w.focused_pane().active_tab;
    w.pin_tab(pane, first);
    let second = w.focused_pane().active_tab;
    w.pin_tab(pane, second);
    assert!(w.move_pin(first, 2));
    assert!(w.resize_pin(first, 720., 410.));
    assert!(!w.resize_pin(first, f32::NAN, 410.));
    let restored = Workspace::from_json(&w.to_json()).unwrap();
    assert_eq!(restored.pinned_tabs, w.pinned_tabs);
    assert_eq!(restored.pinned_tabs[0].tab.id, second);
    let mut legacy: serde_json::Value =
        serde_json::from_str(&Workspace::default().to_json()).unwrap();
    legacy.as_object_mut().unwrap().remove("pinned_tabs");
    assert!(Workspace::from_json(&legacy.to_string())
        .unwrap()
        .pinned_tabs
        .is_empty());
    let mut invalid: serde_json::Value = serde_json::from_str(&w.to_json()).unwrap();
    invalid["pinned_tabs"][0]["tab"]["id"] = invalid["spaces"][0]["id"].clone();
    assert!(Workspace::from_json(&invalid.to_string()).is_err());
    invalid = serde_json::from_str(&w.to_json()).unwrap();
    invalid["pinned_tabs"][0]["width"] = serde_json::json!(-1);
    assert!(Workspace::from_json(&invalid.to_string()).is_err());
}

#[test]
fn pinned_emoji_override_round_trips_and_clear_restores_favicon() {
    let mut w = Workspace::default();
    let tab = w.focused_pane().active_tab;
    w.pin_tab(w.focused_pane().id, tab);
    assert!(w.set_pin_icon(tab, Some("🚀")));
    let restored = Workspace::from_json(&w.to_json()).unwrap();
    assert_eq!(restored.pinned_tabs[0].icon.as_deref(), Some("🚀"));
    let mut old: serde_json::Value = serde_json::from_str(&w.to_json()).unwrap();
    old["pinned_tabs"][0]
        .as_object_mut()
        .unwrap()
        .remove("icon");
    assert_eq!(
        Workspace::from_json(&old.to_string()).unwrap().pinned_tabs[0].icon,
        None
    );
    assert!(w.set_pin_icon(tab, None));
    assert_eq!(
        Workspace::from_json(&w.to_json()).unwrap().pinned_tabs[0].icon,
        None
    );
}

#[test]
fn floating_detach_merge_split_preserve_identity_and_round_trip() {
    let mut w = Workspace::default();
    let tiled = w.focused_pane().id;
    let tab = w.focused_pane().active_tab;
    w.add_tab("https://example.com");
    let bounds = Rect::new(120., 140., 560., 420.);
    let floating = w.float_tab(tiled, tab, bounds, None).unwrap();
    assert_eq!(w.active_space().layout.pane_ids(), vec![tiled]);
    assert_eq!(w.pane(floating).unwrap().active_tab, tab);
    for edge in [
        SplitEdge::Left,
        SplitEdge::Right,
        SplitEdge::Top,
        SplitEdge::Bottom,
    ] {
        assert!(w.float_pane(floating, bounds, Some(edge)));
        assert!(w.pin_floating(floating));
        let restored = Workspace::from_json(&w.to_json()).unwrap();
        assert_eq!(restored, w);
        assert_eq!(restored.active_space().floating[0].edge, Some(edge));
    }
    assert!(w.move_pane(floating, tiled, Some(SplitEdge::Right)));
    assert!(w.active_space().floating.is_empty());
    assert_eq!(w.pane(floating).unwrap().active_tab, tab);
    assert!(w.float_pane(floating, bounds, None));
    assert!(w.move_pane(floating, tiled, None));
    assert!(w.active_space().floating.is_empty());
    assert!(w.pane(tiled).unwrap().tabs.iter().any(|t| t.id == tab));
    assert_eq!(Workspace::from_json(&w.to_json()).unwrap(), w);
}

#[test]
fn final_tiled_stack_detaches_without_losing_a_page_and_stays_space_owned() {
    let mut w = Workspace::default();
    let pane = w.focused_pane().id;
    let tab = w.focused_pane().active_tab;
    let home = w.active_space;
    assert!(w.float_pane(
        pane,
        Rect::new(900., 800., 560., 420.),
        Some(SplitEdge::Right)
    ));
    let replacement = w.active_space().layout.pane_ids()[0];
    assert_ne!(replacement, pane);
    assert_eq!(w.pane(pane).unwrap().active_tab, tab);
    let fitted = w.active_space().floating[0].fitted(Rect::new(6., 44., 600., 400.));
    assert_eq!(fitted, Rect::new(46., 44., 560., 400.));
    let other = w.add_space("Other", "about:blank");
    assert!(w.active_space().floating.is_empty());
    assert!(!w.float_pane(pane, Rect::new(0., 0., 560., 420.), None));
    w.active_space = home;
    let new_tab = w.pane(replacement).unwrap().active_tab;
    assert!(w.move_tab(replacement, new_tab, pane, 1));
    assert!(w
        .active_space()
        .pane(w.active_space().layout.pane_ids()[0])
        .is_some());
    assert_eq!(Workspace::from_json(&w.to_json()).unwrap(), w);
    w.active_space = other;
    assert!(w.active_space().floating.is_empty());
}

#[test]
fn floating_chips_reorder_close_and_validate_snapshot_geometry() {
    let mut w = Workspace::default();
    let a = w.focused_pane().id;
    let b = w.split_focused(Axis::Horizontal, "about:blank");
    let r = Rect::new(100., 100., 500., 350.);
    w.float_pane(a, r, Some(SplitEdge::Left));
    w.float_pane(b, r, Some(SplitEdge::Left));
    assert!(w.reorder_floating(b, 0));
    assert_eq!(
        w.active_space()
            .floating
            .iter()
            .map(|f| f.pane)
            .collect::<Vec<_>>(),
        vec![b, a]
    );
    assert!(!w.resize_floating(a, Rect::new(f32::NAN, 0., 500., 350.)));
    assert!(w.close_pane(a));
    assert_eq!(w.active_space().floating.len(), 1);
    let mut value: serde_json::Value = serde_json::from_str(&w.to_json()).unwrap();
    value["spaces"][0]["floating"][0]["bounds"]["width"] = 10.into();
    assert!(Workspace::from_json(&value.to_string()).is_err());
    value = serde_json::from_str(&w.to_json()).unwrap();
    let duplicate = value["spaces"][0]["floating"][0].clone();
    value["spaces"][0]["floating"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    assert!(Workspace::from_json(&value.to_string()).is_err());
    assert_eq!(Workspace::from_json(&w.to_json()).unwrap(), w);
}

#[test]
fn closing_the_last_tiled_pane_with_overlays_keeps_valid_focus() {
    let mut w = Workspace::default();
    let overlay = w.focused_pane().id;
    w.float_pane(overlay, Rect::new(0., 0., 500., 350.), None);
    let tiled = w.active_space().layout.pane_ids()[0];
    w.focus_pane(tiled);
    assert!(w.close_pane(tiled));
    assert!(w
        .active_space()
        .pane(w.active_space().focused_pane)
        .is_some());
    assert_eq!(Workspace::from_json(&w.to_json()).unwrap(), w);
}

#[test]
fn docked_chips_follow_the_saved_anchor_and_hover_panels_fit() {
    let mut w = Workspace::default();
    let pane = w.focused_pane().id;
    w.float_pane(
        pane,
        Rect::new(500., 610., 420., 300.),
        Some(SplitEdge::Left),
    );
    let a = Rect::new(6., 44., 1000., 740.);
    let p = w.active_space().floating_placements(a)[0];
    assert_eq!(p.chip.unwrap(), Rect::new(6., 610., 28., 156.));
    // The chip keeps the docking anchor while the page shifts up to fit.
    assert_eq!(p.panel.y, 484.);
    assert_eq!(p.panel.y + p.panel.height, 784.);
    assert_eq!(w.active_space().floating[0].bounds.y, 610.);
    let before = w.to_json();
    assert_eq!(
        Workspace::from_json(&before)
            .unwrap()
            .active_space()
            .floating_placements(a)[0]
            .chip,
        p.chip
    );
}

#[test]
fn chip_offsets_avoid_collisions_on_the_same_and_adjacent_edges() {
    let mut w = Workspace::default();
    let first = w.focused_pane().id;
    w.float_pane(first, Rect::new(6., 44., 420., 300.), Some(SplitEdge::Left));
    let second = w.active_space().layout.pane_ids()[0];
    w.float_pane(
        second,
        Rect::new(6., 44., 420., 300.),
        Some(SplitEdge::Left),
    );
    let third = w.active_space().layout.pane_ids()[0];
    w.float_pane(third, Rect::new(6., 44., 420., 300.), Some(SplitEdge::Top));
    for area in [
        Rect::new(6., 44., 1000., 740.),
        Rect::new(6., 44., 500., 330.),
    ] {
        let p = w.active_space().floating_placements(area);
        assert_eq!(p[0].chip.unwrap().y, 44.);
        for (i, a) in p.iter().enumerate() {
            let a = a.chip.unwrap();
            assert!(
                a.x >= area.x
                    && a.y >= area.y
                    && a.x + a.width <= area.x + area.width
                    && a.y + a.height <= area.y + area.height
            );
            for b in &p[i + 1..] {
                let b = b.chip.unwrap();
                assert!(
                    a.x + a.width <= b.x
                        || b.x + b.width <= a.x
                        || a.y + a.height <= b.y
                        || b.y + b.height <= a.y,
                    "overlapping chips: {a:?} {b:?}"
                );
            }
        }
    }
}

#[test]
fn pinned_spaces_partition_reorder_and_restore_without_changing_selection() {
    let mut w = Workspace::default();
    let a = w.spaces[0].id;
    let b = w.add_space("B", "about:blank");
    let c = w.add_space("C", "about:blank");
    let d = w.add_space("D", "about:blank");
    let order = |w: &Workspace| w.spaces.iter().map(|s| s.id).collect::<Vec<_>>();
    w.set_space_details(b, "Books", Some("📚"));
    assert!(w.set_space_pinned(b, true));
    assert!(w.set_space_pinned(c, true));
    assert_eq!(order(&w), [b, c, a, d]);
    assert_eq!(w.active_space, d);
    assert!(!w.set_space_pinned(b, true));
    w.move_space(b, usize::MAX);
    assert_eq!(order(&w), [c, b, a, d]);
    w.move_space(d, 0);
    assert_eq!(order(&w), [c, b, d, a]);
    assert!(w.spaces[..2].iter().all(|s| s.pinned));
    assert!(w.spaces[2..].iter().all(|s| !s.pinned));
    assert_eq!(Workspace::from_json(&w.to_json()).unwrap(), w);
    w.set_space_pinned(c, false);
    assert_eq!(order(&w), [b, c, d, a]);
    assert_eq!(w.spaces[0].icon.as_deref(), Some("📚"));
    assert_eq!(w.spaces[0].display_title(), "Books");
    assert_eq!(w.active_space, d);
    let mut old: serde_json::Value = serde_json::from_str(&w.to_json()).unwrap();
    for space in old["spaces"].as_array_mut().unwrap() {
        space.as_object_mut().unwrap().remove("pinned");
    }
    let legacy = Workspace::from_json(&old.to_string()).unwrap();
    assert!(legacy.spaces.iter().all(|s| !s.pinned));
    assert_eq!(order(&legacy), order(&w));
}
