//! Native AppKit click coverage, with Cmd+Q driven by scripts/test-macos-input.py.
use crate::{
    app::TabTarget,
    smoke::{eval, pause, wait_title},
};
use anyhow::{Result, ensure};
use gpui::*;
use workspace_core::TabId;

pub async fn run(
    view: &WeakEntity<crate::app::BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (host, source, pane, following) = view.update_in(cx, |app, _, _| {
        let source = app.active_tab();
        let pane = app.model.focused_pane().id;
        let following = app.model.add_tab("about:blank");
        (app.host.clone(), source, pane, following)
    })?;
    for (name, modified, target_blank) in [
        ("cmd-link", true, false),
        ("cmd-blank-link", true, true),
        ("ordinary-blank-link", false, true),
    ] {
        view.update_in(cx, |app, window, cx| {
            app.select_target(TabTarget::Page(pane, source), window, cx);
        })?;
        pause(cx).await;
        let script = format!(
            r#"(() => {{
            window.axolotlSource = 'preserved';
            window.axolotlEvents = [];
            for (const type of ['pointerdown', 'mouseup', 'click']) window.addEventListener(type, e => window.axolotlEvents.push([type, e.clientX, e.clientY, e.metaKey, e.isTrusted, e.target.outerHTML]), true);
            document.body.innerHTML = '<a href="about:blank" {} style="position:fixed;left:20px;top:20px;width:200px;height:80px;display:block"><span>Native link</span></a>';
            document.body.style.margin = '0';
            return true;
        }})()"#,
            if target_blank {
                "target=\"_blank\""
            } else {
                ""
            }
        );
        eval(&host, source, &script, cx).await?;
        host.focus(source)?;
        let before = view.update_in(cx, |app, _, _| app.model.pane(pane).unwrap().tabs.len())?;
        host.test_click(source, modified)?;
        let mut created = None;
        for _ in 0..50 {
            pause(cx).await;
            created = view.update_in(cx, |app, _, _| {
                let pane = app.model.pane(pane).unwrap();
                (pane.tabs.len() == before + 1).then_some(pane.active_tab)
            })?;
            if created.is_some() {
                break;
            }
        }
        let created: TabId = match created {
            Some(tab) => tab,
            None => anyhow::bail!(
                "{name}: no new tab; events: {}",
                eval(&host, source, "window.axolotlEvents", cx).await?
            ),
        };
        // Let any duplicate native popup event arrive before counting again.
        for _ in 0..3 {
            pause(cx).await;
        }
        view.update_in(cx, |app, _, _| -> Result<()> {
            let pane = app.model.pane(pane).unwrap();
            ensure!(pane.tabs.len() == before + 1, "{name}: duplicate tab");
            ensure!(
                pane.tabs.iter().any(|t| t.id == following),
                "following tab lost"
            );
            if modified {
                let index = pane.tabs.iter().position(|t| t.id == source).unwrap();
                ensure!(pane.tabs[index + 1].id == created, "{name}: not a sibling");
            } else {
                ensure!(
                    pane.tabs.last().unwrap().id == created,
                    "popup append behavior changed"
                );
            }
            Ok(())
        })??;
        ensure!(
            eval(&host, source, "window.axolotlSource", cx).await? == "preserved",
            "{name}: source navigated"
        );
    }
    view.update_in(cx, |app, window, cx| {
        app.select_target(TabTarget::Page(pane, source), window, cx);
    })?;
    pause(cx).await;
    eval(
        &host,
        source,
        r#"document.querySelector('a').removeAttribute('target'); true"#,
        cx,
    )
    .await?;
    let before = view.update_in(cx, |app, _, _| app.model.pane(pane).unwrap().tabs.len())?;
    host.test_click(source, false)?;
    for _ in 0..50 {
        pause(cx).await;
        if eval(&host, source, "window.axolotlSource !== 'preserved'", cx).await? == true {
            break;
        }
    }
    ensure!(
        eval(&host, source, "window.axolotlSource !== 'preserved'", cx).await? == true,
        "ordinary link failed to navigate source"
    );
    view.update_in(cx, |app, _, _| -> Result<()> {
        ensure!(
            app.model.pane(pane).unwrap().tabs.len() == before,
            "ordinary click created a tab"
        );
        Ok(())
    })??;
    host.navigate(source, "about:blank")?;
    wait_title(&host, source, "New tab · Axolotl", cx).await?;
    host.focus(source)?;
    println!(
        "AXOLOTL_INPUT {}",
        serde_json::json!({"action":"quit", "pid":std::process::id()})
    );
    // A successful Cmd+Q terminates through GPUI; reaching this deadline fails.
    for _ in 0..50 {
        pause(cx).await;
    }
    anyhow::bail!("Cmd+Q did not terminate the app with web content focused")
}
