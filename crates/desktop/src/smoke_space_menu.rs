//! Exercise the real native space menu and IPC without changing the active space.
use crate::{app::BrowserApp, smoke::pause};
use anyhow::{Result, ensure};
use gpui::*;
use serde_json::Value;
use std::sync::{Arc, Mutex};

async fn eval(
    view: &WeakEntity<BrowserApp>,
    script: &str,
    cx: &mut AsyncWindowContext,
) -> Result<Value> {
    let response = Arc::new(Mutex::new(None));
    let target = response.clone();
    view.update_in(cx, |app, _, _| {
        app.space_menu
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing space menu"))?
            .evaluate(script, move |v| *target.lock().unwrap() = Some(v))
    })??;
    for _ in 0..50 {
        if let Some(value) = response.lock().unwrap().take() {
            return Ok(serde_json::from_str(&value)?);
        }
        pause(cx).await;
    }
    anyhow::bail!("Space menu evaluation timed out")
}

pub(crate) async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<&'static str> {
    let (active, other) = view.update_in(cx, |app, _, _| {
        let active = app.model.active_space;
        let other = app.model.add_space("Menu target", "about:blank");
        app.model.switch_space(active);
        (active, other)
    })?;
    for action in ["save", "clear", "escape", "outside"] {
        view.update_in(cx, |app, window, cx| {
            let size = window.viewport_size();
            // Near both viewport edges; the menu/editor must remain inside them.
            app.open_space_menu(
                other,
                point(size.width - px(2.), size.height - px(2.)),
                window,
                cx,
            );
        })?;
        pause(cx).await;
        pause(cx).await;
        let snapshot = eval(view, "(()=>{const r=menu.getBoundingClientRect();return {items:[...menu.querySelectorAll('[role=menuitem]')].map(b=>b.textContent),fits:r.left>=0&&r.top>=0&&r.right<=innerWidth&&r.bottom<=innerHeight}})()", cx).await?;
        ensure!(
            snapshot["items"] == serde_json::json!(["Set title…"]) && snapshot["fits"] == true,
            "Incorrect/clipped space menu: {snapshot}"
        );
        match action {
            "save" | "clear" => {
                let editor = eval(view, "button.click();({focused:document.activeElement===input,selected:input.selectionEnd-input.selectionStart,fits:menu.getBoundingClientRect().bottom<=innerHeight})", cx).await?;
                ensure!(
                    editor["focused"] == true && editor["fits"] == true,
                    "Space title editor is not usable: {editor}"
                );
                let value = if action == "save" {
                    "  Fixed space  "
                } else {
                    ""
                };
                eval(
                    view,
                    &format!("input.value={value:?};form.requestSubmit();true"),
                    cx,
                )
                .await?;
            }
            "escape" => {
                eval(view, "document.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));true", cx).await?;
            }
            _ => {
                eval(view, "document.body.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true}));true", cx).await?;
            }
        }
        pause(cx).await;
        view.update_in(cx, |app, _, _| {
            ensure!(app.space_menu.is_none(), "Space menu failed to dismiss");
            ensure!(
                app.model.active_space == active,
                "Context menu switched spaces"
            );
            let space = app.model.spaces.iter().find(|s| s.id == other).unwrap();
            ensure!(
                space.custom_title.as_deref()
                    == if action == "save" {
                        Some("Fixed space")
                    } else {
                        None
                    },
                "Space title targeted the wrong space or failed to clear"
            );
            ensure!(
                app.model.active_space().custom_title.is_none(),
                "Changed active space title"
            );
            Ok(())
        })??;
    }
    view.update_in(cx, |app, _, _| {
        app.model.close_space(other);
    })?;
    Ok(
        "native space menu: single Set title item, inactive-space targeting, save/clear, focused editor, viewport clamping, Escape/outside dismissal",
    )
}
