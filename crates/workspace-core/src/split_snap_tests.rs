use super::*;

fn center(divider: Divider) -> (f32, f32) {
    let b = divider.bounds;
    (b.x + b.width / 2., b.y + b.height / 2.)
}

fn close(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.001, "{actual} != {expected}");
}

#[test]
fn overlapping_snap_ranges_choose_the_nearest_stop() {
    let divider = Divider {
        id: 1,
        axis: Axis::Horizontal,
        bounds: Rect::new(497., 0., 6., 600.),
        container: Rect::new(0., 0., 1000., 600.),
    };
    // Both quarter and third are within 50px; the closest one wins.
    for (pointer, stop) in [(285., 250.), (300., 1000. / 3.)] {
        close(divider.drag_ratio(pointer, 0.) * 994. + 3., stop);
    }
}

#[test]
fn nested_third_accepts_exact_fifty_pixel_boundary_without_cancellation() {
    for axis in [Axis::Horizontal, Axis::Vertical] {
        let divider = Divider {
            id: 1,
            axis,
            bounds: Rect::new(1047., 1047., 6., 6.),
            container: Rect::new(703., 703., 1691., 1691.),
        };
        let stop = 703. + 1691. / 3.;
        for offset in [-50., 50.] {
            let ratio = divider.drag_ratio(stop + offset, stop + offset);
            close(703. + ratio * 1685. + 3., stop);
        }
    }
}

#[test]
fn all_stops_snap_by_parent_center_in_both_axes_and_release_freely() {
    for axis in [Axis::Horizontal, Axis::Vertical] {
        let mut layout = Layout::Split {
            id: 1,
            axis,
            ratio: 0.5,
            first: Box::new(Layout::Pane(2)),
            second: Box::new(Layout::Pane(3)),
        };
        let bounds = Rect::new(123., 217., 2400., 2400.);
        let divider = layout.geometry(bounds, 6., 80.).dividers[0];
        for fraction in [0.25, 1. / 3., 0.5, 2. / 3., 0.75] {
            let stop = 2400. * fraction;
            for offset in [-50.25, -50., -25., 0., 25., 50., 50.25] {
                let position = stop + offset;
                let (x, y) = match axis {
                    Axis::Horizontal => (bounds.x + position, bounds.y + 50.),
                    Axis::Vertical => (bounds.x + 50., bounds.y + position),
                };
                assert!(layout.resize(1, divider.drag_ratio(x, y)));
                let placed = layout.geometry(bounds, 6., 80.).dividers[0];
                let (x, y) = center(placed);
                let actual = match axis {
                    Axis::Horizontal => x - bounds.x,
                    Axis::Vertical => y - bounds.y,
                };
                close(actual, if offset.abs() <= 50. { stop } else { position });
            }
        }
    }
}

#[test]
fn nested_snap_uses_its_own_parent_with_nonstandard_gutter() {
    let mut workspace = Workspace::default();
    workspace.grid("about:blank");
    let bounds = Rect::new(71., 113., 1400., 1100.);
    let geometry = workspace.active_space().layout.geometry(bounds, 12., 80.);
    for divider in geometry.dividers.into_iter().skip(1) {
        for fraction in [0.25, 1. / 3., 0.5, 2. / 3., 0.75] {
            let parent = divider.container;
            let x = parent.x + parent.width * fraction;
            let y = parent.y + parent.height * fraction;
            for offset in [-5., 0., 5.] {
                workspace.resize_split(divider.id, divider.drag_ratio(x + offset, y + offset));
                let placed = workspace.active_space().layout.geometry(bounds, 12., 80.);
                let placed = *placed.dividers.iter().find(|d| d.id == divider.id).unwrap();
                let (actual_x, actual_y) = center(placed);
                match divider.axis {
                    Axis::Horizontal => close(actual_x, x),
                    Axis::Vertical => close(actual_y, y),
                }
            }
        }
    }
}

#[test]
fn snapping_preserves_recursive_minimums_and_undersized_layouts() {
    let mut workspace = Workspace::default();
    workspace.grid("about:blank");
    for size in [0., 4., 100., 300., 500.] {
        let bounds = Rect::new(23., 47., size, size);
        let before = workspace.active_space().layout.geometry(bounds, 6., 120.);
        for divider in before.dividers {
            let x = divider.container.x + divider.container.width * 0.25;
            let y = divider.container.y + divider.container.height * 0.25;
            workspace.resize_split(divider.id, divider.drag_ratio(x, y));
        }
        let after = workspace.active_space().layout.geometry(bounds, 6., 120.);
        for pane in after.panes {
            assert!(pane.bounds.width.is_finite() && pane.bounds.width >= 0.);
            assert!(pane.bounds.height.is_finite() && pane.bounds.height >= 0.);
            if size >= 246. {
                assert!(pane.bounds.width >= 120.);
                assert!(pane.bounds.height >= 120.);
            }
        }
    }
}
