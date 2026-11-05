//! Inspect real Core Animation presentation layers, not only configured styles.
use crate::{
    app::{BrowserApp, Command},
    link_popup::POPUP_TAB,
    popup_transition,
};
use anyhow::{Context as _, Result, ensure};
use gpui::*;
use serde_json::Value;
use std::time::{Duration, Instant};

struct MotionOverride;
impl Drop for MotionOverride {
    fn drop(&mut self) {
        popup_transition::set_test_reduced_motion(None);
    }
}

#[derive(Debug)]
struct Surfaces {
    page: Value,
    backdrop: Value,
}

impl Surfaces {
    fn animating(&self) -> bool {
        self.page["animating"] == true && self.backdrop["animating"] == true
    }
    fn settled(&self) -> bool {
        [&self.page, &self.backdrop]
            .iter()
            .all(|s| s["model"] == 1. && s["animating"] == false)
    }
    fn opacity(&self) -> Option<(f64, f64)> {
        Some((
            self.page["presentation"].as_f64()?,
            self.backdrop["presentation"].as_f64()?,
        ))
    }
}

async fn tick(cx: &mut AsyncWindowContext, millis: u64) -> Result<()> {
    cx.background_executor()
        .timer(Duration::from_millis(millis))
        .await;
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })?;
    Ok(())
}

fn surfaces(view: &WeakEntity<BrowserApp>, cx: &mut AsyncWindowContext) -> Result<Surfaces> {
    view.update_in(cx, |app, _, _| -> Result<_> {
        let popup = app.link_popup.as_ref().context("Missing popup")?;
        Ok(Surfaces {
            page: app.host.popup_animation_state(POPUP_TAB)?,
            backdrop: popup_transition::state(popup.backdrop.as_ref().context("Missing backdrop")?),
        })
    })?
}

async fn open(view: &WeakEntity<BrowserApp>, cx: &mut AsyncWindowContext) -> Result<Surfaces> {
    view.update_in(cx, |app, window, cx| {
        app.open_link_popup(app.active_tab(), "about:blank".into(), window, cx)
    })?;
    for _ in 0..100 {
        tick(cx, 10).await?;
        if view.update_in(cx, |app, _, _| {
            app.link_popup.as_ref().is_some_and(|p| p.presented)
        })? {
            return surfaces(view, cx);
        }
    }
    anyhow::bail!("Popup did not present")
}

async fn settle(view: &WeakEntity<BrowserApp>, cx: &mut AsyncWindowContext) -> Result<()> {
    for _ in 0..60 {
        tick(cx, 10).await?;
        if surfaces(view, cx)?.settled() {
            return Ok(());
        }
    }
    anyhow::bail!("Popup fade did not settle: {:?}", surfaces(view, cx)?)
}

async fn wait_disposed(view: &WeakEntity<BrowserApp>, cx: &mut AsyncWindowContext) -> Result<()> {
    for _ in 0..60 {
        tick(cx, 10).await?;
        if view.update_in(cx, |app, _, _| {
            app.link_popup.is_none() && !app.host.has_view(POPUP_TAB)
        })? {
            return Ok(());
        }
    }
    anyhow::bail!("Closing fade did not dispose the native popup")
}

pub async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    popup_transition::set_test_reduced_motion(Some(false));
    let _motion = MotionOverride;
    let first = open(view, cx).await?;
    ensure!(
        first.animating(),
        "Opening did not install animations on both native surfaces: {first:?}"
    );
    ensure!(
        first.page["model"] == 1. && first.backdrop["model"] == 1.,
        "Opening model alpha did not target opaque: {first:?}"
    );
    let mut opening_sample = None;
    for _ in 0..15 {
        tick(cx, 10).await?;
        let state = surfaces(view, cx)?;
        if let Some((page, backdrop)) = state.opacity()
            && page > 0.001
            && page < 0.98
            && backdrop > 0.001
            && backdrop < 0.98
        {
            opening_sample = Some((page, backdrop));
            break;
        }
    }
    let (page_before, backdrop_before) =
        opening_sample.context("Opening never showed intermediate native opacity")?;
    tick(cx, 20).await?;
    let opening_after = surfaces(view, cx)?;
    let (page_after, backdrop_after) = opening_after
        .opacity()
        .context("Opening presentation layers unavailable")?;
    ensure!(
        page_after > page_before && backdrop_after > backdrop_before,
        "Opening did not progress on both native surfaces: before={page_before}/{backdrop_before} after={opening_after:?}"
    );
    settle(view, cx).await?;

    let deadline = view.update_in(cx, |app, window, cx| -> Result<_> {
        app.dismiss_link_popup(window, cx);
        let deadline = app
            .link_popup
            .as_ref()
            .and_then(|p| p.closing_until)
            .context("Natural close disposed before transition")?;
        ensure!(
            app.host.has_view(POPUP_TAB),
            "Natural close removed native page before fading"
        );
        app.dismiss_link_popup(window, cx);
        ensure!(
            app.link_popup.as_ref().unwrap().closing_until == Some(deadline),
            "Repeated dismissal extended the close deadline"
        );
        Ok(deadline)
    })??;
    let closing = surfaces(view, cx)?;
    ensure!(
        closing.animating() && closing.page["model"] == 0. && closing.backdrop["model"] == 0.,
        "Closing did not animate both native surfaces toward transparent: {closing:?}"
    );
    tick(cx, 30).await?;
    let closing_middle = surfaces(view, cx)?;
    let (page, backdrop) = closing_middle
        .opacity()
        .context("Closing presentation layers unavailable")?;
    ensure!(
        page > 0. && page < 1. && backdrop > 0. && backdrop < 1.,
        "Closing never displayed intermediate alpha: {closing_middle:?}"
    );
    ensure!(
        Instant::now() < deadline,
        "Closing sample missed its retention interval"
    );
    wait_disposed(view, cx).await?;

    // Reusing the popup during its fade must cancel the old destruction timer.
    open(view, cx).await?;
    settle(view, cx).await?;
    let old_deadline = view.update_in(cx, |app, window, cx| {
        app.dismiss_link_popup(window, cx);
        app.link_popup.as_ref().unwrap().closing_until.unwrap()
    })?;
    tick(cx, 25).await?;
    let reopened = open(view, cx).await?;
    ensure!(
        reopened.page["model"] == 1. && reopened.backdrop["model"] == 1.,
        "Reopening did not reverse the fade: {reopened:?}"
    );
    view.update_in(cx, |app, _, _| -> Result<()> {
        ensure!(
            app.link_popup.as_ref().unwrap().closing_until.is_none(),
            "Reopening kept the stale close deadline"
        );
        Ok(())
    })??;
    while Instant::now() <= old_deadline + Duration::from_millis(80) {
        tick(cx, 10).await?;
    }
    settle(view, cx).await?;

    // Promotion in the middle of closing must remove opacity animation/state.
    let promoted = view.update_in(cx, |app, window, cx| -> Result<_> {
        app.dismiss_link_popup(window, cx);
        ensure!(
            app.host.popup_animation_state(POPUP_TAB)?["model"] == 0.,
            "Promotion fixture was not closing"
        );
        app.promote_link_popup(window, cx);
        ensure!(
            app.link_popup.is_none() && !app.host.has_view(POPUP_TAB),
            "Promotion retained transient ownership"
        );
        Ok(app.active_tab())
    })??;
    let promotion = view.update_in(cx, |app, _, _| app.host.popup_animation_state(promoted))??;
    ensure!(
        promotion["model"] == 1. && promotion["animating"] == false,
        "Promoted page retained a faded native layer: {promotion}"
    );
    tick(cx, 200).await?;
    view.update_in(cx, |app, _, _| -> Result<()> {
        ensure!(
            app.host.has_view(promoted) && app.model.tab(promoted).is_some(),
            "Stale close removed the promoted page"
        );
        Ok(())
    })??;

    // Opening UI that replaces the overlay must complete cleanup synchronously.
    open(view, cx).await?;
    view.update_in(cx, |app, window, cx| -> Result<()> {
        app.command(Command::NewTab, window, cx);
        ensure!(
            app.link_popup.is_none() && !app.host.has_view(POPUP_TAB),
            "NewTab left a fading overlay over its editor"
        );
        ensure!(app.editing.is_some(), "NewTab lost address editing");
        Ok(())
    })??;
    open(view, cx).await?;
    view.update_in(cx, |app, window, cx| -> Result<()> {
        app.dismiss_link_popup(window, cx);
        app.open_settings(false, window, cx);
        ensure!(
            app.settings.is_some() && app.link_popup.is_none() && !app.host.has_view(POPUP_TAB),
            "Settings retained a fading overlay above itself"
        );
        app.close_settings(window, cx);
        Ok(())
    })??;

    // Reduce Motion removes transitions and their disposal delay entirely.
    popup_transition::set_test_reduced_motion(Some(true));
    let reduced = open(view, cx).await?;
    ensure!(
        reduced.settled(),
        "Reduce Motion still animated native popup surfaces: {reduced:?}"
    );
    view.update_in(cx, |app, window, cx| -> Result<()> {
        app.dismiss_link_popup(window, cx);
        ensure!(
            app.link_popup.is_none() && !app.host.has_view(POPUP_TAB),
            "Reduce Motion retained delayed disposal"
        );
        Ok(())
    })??;
    Ok(vec![
        "actual native page and backdrop presentation opacity progresses during opening and closing",
        "natural close retains surfaces through its deadline then disposes them; repeated dismissal does not extend it",
        "reopening cancels stale disposal; promotion resets native alpha/animation and retains the regular page",
        "new-tab and Settings replacement dispose overlays immediately; Reduce Motion skips animation and delayed disposal",
    ])
}
