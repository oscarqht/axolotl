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
            .ok_or_else(|| anyhow::anyhow!("Missing space menu evaluating: {script}"))?
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
    let (active, other, page_titles) = view.update_in(cx, |app, _, _| {
        let active = app.model.active_space;
        let other = app.model.add_space("Menu target", "about:blank");
        // Give the target page a distinct title so writing the space title onto
        // its active page cannot accidentally satisfy the assertions below.
        let tab = app.model.focused_pane().active_tab;
        app.model.tab_mut(tab).unwrap().title = "Original page title".into();
        app.model.set_custom_title(tab, "Fixed page title");
        app.model.switch_space(active);
        let mut page_titles: Vec<_> = app
            .model
            .spaces
            .iter()
            .flat_map(|s| &s.panes)
            .flat_map(|p| &p.tabs)
            .map(|t| (t.id, t.custom_title.clone()))
            .collect();
        page_titles.sort_by_key(|t| t.0);
        (active, other, page_titles)
    })?;
    for expected in [true, false] {
        view.update_in(cx, |app, window, cx| {
            app.open_space_menu(other, point(px(110.), px(35.)), window, cx);
        })?;
        pause(cx).await;
        pause(cx).await;
        let label = eval(view, "document.querySelector('#pin-space').textContent", cx).await?;
        ensure!(
            label == if expected { "Pin Space" } else { "Unpin Space" },
            "Wrong pin label: {label}"
        );
        eval(
            view,
            "document.querySelector('#pin-space').click();true",
            cx,
        )
        .await?;
        pause(cx).await;
        view.update_in(cx, |app, _, _| -> Result<()> {
            ensure!(
                app.model
                    .spaces
                    .iter()
                    .find(|s| s.id == other)
                    .unwrap()
                    .pinned
                    == expected,
                "Pin IPC failed"
            );
            ensure!(
                app.model.active_space == active,
                "Pin action changed selection"
            );
            Ok(())
        })??;
    }
    // Let newly created native page surfaces settle before mounting the menu.
    pause(cx).await;
    pause(cx).await;
    for action in ["save", "escape", "outside", "enter", "clear", "blank"] {
        view.update_in(cx, |_, window, cx| {
            window.activate_window();
            cx.activate(true);
        })?;
        pause(cx).await;
        pause(cx).await;
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
        let snapshot = eval(view, "(()=>{const r=menu.getBoundingClientRect();const rows=[...menu.querySelectorAll('[role=menuitem]')];return {fullWidth:rows.every(b=>Math.abs(b.offsetWidth-(menu.clientWidth-10))<=1),hoverOnly:rows.every(b=>b.matches(':hover')||getComputedStyle(b).backgroundColor==='rgba(0, 0, 0, 0)'),items:rows.map(b=>b.textContent),fits:r.left>=0&&r.top>=0&&r.right<=innerWidth&&r.bottom<=innerHeight}})()", cx).await?;
        ensure!(
            snapshot["items"] == serde_json::json!(["Pin Space", "Sleep Space", "Edit space"])
                && snapshot["fits"] == true
                && snapshot["fullWidth"] == true
                && snapshot["hoverOnly"] == true,
            "Incorrect/clipped space menu: {snapshot}"
        );
        let editor = eval(view, "button.click();({focused:document.activeElement===input,fits:menu.getBoundingClientRect().bottom<=innerHeight,icon:icon})", cx).await?;
        ensure!(
            editor["focused"] == true && editor["fits"] == true,
            "Editor unusable: {editor}"
        );
        ensure!(
            editor["icon"]
                == if action == "save" || action == "blank" {
                    Value::Null
                } else {
                    serde_json::json!("🚀")
                },
            "Wrong initial icon: {editor}"
        );
        let picker = eval(view, "iconButton.click();search.value='rocket';search.dispatchEvent(new Event('input'));(()=>{const r=menu.getBoundingClientRect();return {rocket:[...emojis.children].some(b=>b.textContent==='🚀'),fits:r.left>=0&&r.top>=0&&r.right<=innerWidth&&r.bottom<=innerHeight,focused:document.activeElement===search}})()", cx).await?;
        ensure!(
            picker["rocket"] == true && picker["fits"] == true && picker["focused"] == true,
            "Picker failed: {picker}"
        );
        let selected = eval(view, "[...emojis.children].find(b=>b.textContent==='🚀').click();({icon,closed:picker.hidden})", cx).await?;
        ensure!(
            selected["icon"] == "🚀" && selected["closed"] == true,
            "Selection failed: {selected}"
        );
        match action {
            "save" => {
                eval(
                    view,
                    "input.value='  Fixed space  ';document.querySelector('#save').click();true",
                    cx,
                )
                .await?;
            }
            "enter" => {
                eval(view, "input.value='Fixed space';input.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));true", cx).await?;
            }
            "clear" => {
                let cleared = eval(view, "iconButton.click();search.value='zzzz-no-match';search.dispatchEvent(new Event('input'));(()=>{const empty=!document.querySelector('#empty').hidden&&emojis.children.length===0;document.querySelector('#clear').click();return {empty,icon,closed:picker.hidden}})()", cx).await?;
                ensure!(
                    cleared["empty"] == true
                        && cleared["icon"].is_null()
                        && cleared["closed"] == true,
                    "Clear failed: {cleared}"
                );
                eval(view, "form.requestSubmit();true", cx).await?;
            }
            "blank" => {
                eval(view, "input.value=' ';form.requestSubmit();true", cx).await?;
            }
            "escape" => {
                eval(view, "input.value='Discard me';iconButton.click();document.querySelector('#clear').click();document.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));true", cx).await?;
            }
            _ => {
                eval(view, "input.value='Discard me';iconButton.click();document.querySelector('#clear').click();document.body.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true}));true", cx).await?;
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
                    == if action != "blank" {
                        Some("Fixed space")
                    } else {
                        None
                    },
                "Space title targeted the wrong space or failed to clear"
            );
            ensure!(
                space.icon.as_deref()
                    == if action == "clear" {
                        None
                    } else {
                        Some("🚀")
                    },
                "Wrong icon after {action}"
            );
            let restored = workspace_core::Workspace::from_json(&app.model.to_json())?;
            let restored_space = restored.spaces.iter().find(|s| s.id == other).unwrap();
            ensure!(
                restored_space.icon == space.icon
                    && restored_space.custom_title == space.custom_title,
                "Space edit did not persist"
            );
            ensure!(
                space.focused_pane().active_tab().title == "Original page title"
                    && space.focused_pane().active_tab().display_title() == "Fixed page title",
                "Space menu changed its active page title"
            );
            let mut current_page_titles: Vec<_> = app
                .model
                .spaces
                .iter()
                .flat_map(|s| &s.panes)
                .flat_map(|p| &p.tabs)
                .map(|t| (t.id, t.custom_title.clone()))
                .collect();
            current_page_titles.sort_by_key(|t| t.0);
            ensure!(
                current_page_titles == page_titles,
                "Space menu changed a page tab custom title"
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
        "native Edit space: emoji search/select/clear, no results, Save/Enter, draft cancellation, title fallback, session roundtrip, inactive targeting, unchanged page titles, viewport clamping",
    )
}

/// Multi-process persistence regression, isolated from the user's profile.
pub(crate) fn session_probe(app: &Entity<BrowserApp>, window: &Window, cx: &mut App) {
    let Some(stage) =
        std::env::args().find_map(|a| a.strip_prefix("--space-session-probe=").map(str::to_owned))
    else {
        return;
    };
    assert!(
        std::env::var_os("AXOLOTL_DATA_DIR").is_some(),
        "Session probe requires an isolated profile"
    );
    let view = app.downgrade();
    window.spawn(cx, async move |cx| {
        let result: Result<Value> = async {
            pause(cx).await;
            pause(cx).await;
            view.update_in(cx, |app, window, cx| {
                window.activate_window();
                cx.activate(true);
                let _ = app;
            })?;
            pause(cx).await;
            pause(cx).await;
            view.update_in(cx, |app, window, cx| {
                app.open_space_menu(app.model.active_space, point(px(120.), px(40.)), window, cx);
            })?;
            pause(cx).await;
            pause(cx).await;
            let before = eval(&view, "button.click();({title:input.value,icon})", cx).await?;
            if stage == "seed" {
                eval(&view, "input.value='Restart space';iconButton.click();search.value='rocket';search.dispatchEvent(new Event('input'));[...emojis.children].find(b=>b.textContent==='🚀').click();document.querySelector('#save').click();true", cx).await?;
            } else {
                ensure!(before["title"] == "Restart space", "Title not restored: {before}");
                ensure!(before["icon"] == if stage == "restore" { serde_json::json!("🚀") } else { Value::Null }, "Icon not restored: {before}");
                eval(&view, "document.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));true", cx).await?;
            }
            pause(cx).await;
            if stage == "seed" {
                view.update_in(cx, |app, window, cx| {
                    app.open_space_menu(app.model.active_space, point(px(120.), px(40.)), window, cx);
                })?;
                pause(cx).await;
                pause(cx).await;
                eval(&view, "document.querySelector('#pin-space').click();true", cx).await?;
                pause(cx).await;
            }
            view.update_in(cx, |app, _, _| -> Result<()> {
                ensure!(app.model.active_space().pinned == (stage != "legacy"), "Pin status not saved/restored or legacy default incorrect");
                Ok(())
            })??;
            // Let the real SessionSaver debounce write before process shutdown.
            for _ in 0..6 { pause(cx).await; }
            Ok(before)
        }.await;
        match result {
            Ok(before) => println!("AXOLOTL_SPACE_SESSION {}", serde_json::json!({"stage":stage,"before":before})),
            Err(error) => { eprintln!("Session probe failed: {error:#}"); std::process::exit(1); }
        }
        let _ = cx.update(|_, cx| cx.quit());
    }).detach();
}
