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
fn closing_final_pane_tab_keeps_space_and_invalid_closes_are_atomic() {
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
    assert_eq!(w.active_space, other);
    assert_eq!(w.active_space().layout, Layout::Pane(pane));
    assert_eq!(w.focused_pane().id, pane);
    assert_ne!(w.focused_pane().active_tab, tab);
    assert_eq!(w.focused_pane().active_tab().url, "about:blank");
    assert_eq!(w.focused_pane().active_tab().title, "");
    assert!(w.tab(tab).is_none());
    assert_eq!(
        w.spaces.iter().find(|s| s.id == home),
        before.iter().find(|s| s.id == home)
    );
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
