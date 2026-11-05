//! Regression checks through native AppKit dispatch, not DOM KeyboardEvents.
use crate::smoke::{Host, eval, pause, wait_title};
use anyhow::{Result, ensure};
use gpui::*;
use objc2_app_kit::NSEventModifierFlags as Flags;
use workspace_core::TabId;

fn fixture_url() -> Result<String> {
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let url = format!("http://{}/", listener.local_addr()?);
    // A real HTTP origin permits same-document history navigation. about:blank
    // fragments are intentionally rejected by the production navigation policy.
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
            let _ = stream.read(&mut [0; 4096]);
            let body = "<!doctype html><title>Keyboard fixture</title><body></body>";
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    Ok(url)
}

async fn press(
    host: &Host,
    tab: TabId,
    code: u16,
    text: &str,
    flags: Flags,
    repeat: bool,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    if std::env::args().any(|arg| arg == "--smoke-keyboard-os") {
        let modifiers: Vec<_> = [
            (Flags::Shift, "shift"),
            (Flags::Option, "option"),
            (Flags::Command, "command"),
            (Flags::Control, "control"),
        ]
        .into_iter()
        .filter_map(|(flag, name)| flags.contains(flag).then_some(name))
        .collect();
        println!(
            "AXOLOTL_KEYBOARD {}",
            serde_json::json!({"pid":std::process::id(),"key_code":code,"modifiers":modifiers})
        );
        let key = match text {
            "\u{f700}" => "ArrowUp",
            "\u{f701}" => "ArrowDown",
            "\u{f702}" => "ArrowLeft",
            "\u{f703}" => "ArrowRight",
            "\u{7f}" => "Backspace",
            text => text,
        };
        let delivered = format!(
            "keyEvents.some(e => e[0] === 'keydown' && e[1] === {})",
            serde_json::to_string(key)?
        );
        for _ in 0..50 {
            pause(cx).await;
            if eval(host, tab, &delivered, cx).await? == true {
                return Ok(());
            }
        }
        anyhow::bail!("OS key press did not reach web content");
    }
    host.test_key(tab, code, text, flags, repeat)?;
    pause(cx).await;
    Ok(())
}

pub async fn run(
    view: &WeakEntity<crate::app::BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (host, tab) = view.update_in(cx, |app, _, _| (app.host.clone(), app.active_tab()))?;
    host.navigate(tab, &fixture_url()?)?;
    wait_title(&host, tab, "Keyboard fixture", cx).await?;
    eval(&host, tab, include_str!("../assets/smoke-keyboard.js"), cx).await?;
    eval(&host, tab, "prepare('input')", cx).await?;
    if std::env::args().any(|arg| arg == "--smoke-keyboard-os") {
        // Let the OS runner activate this process before global app shortcuts.
        press(&host, tab, 124, "\u{f703}", Flags::empty(), false, cx).await?;
        eval(&host, tab, "prepare('input')", cx).await?;
    }
    // Browser history shortcuts still work outside editable page controls.
    eval(&host, tab, "editor.blur(); document.body.tabIndex = 0; document.body.focus(); history.pushState({}, '', '#keyboard-one'); history.pushState({}, '', '#keyboard-two'); true", cx).await?;
    host.test_key(
        tab,
        123,
        "\u{f702}",
        Flags::Option | Flags::NumericPad | Flags::Function,
        false,
    )?;
    for _ in 0..20 {
        pause(cx).await;
        if eval(&host, tab, "location.hash", cx).await? == "#keyboard-one" {
            break;
        }
    }
    ensure!(
        eval(&host, tab, "location.hash", cx).await? == "#keyboard-one",
        "Option+Left no longer navigates history outside editors"
    );
    eprintln!("AXOLOTL_KEYBOARD: history check complete");
    // App shortcuts must continue to reach the existing bridge/local monitor.
    // These can consume keydown before the fixture sees it, so dispatch directly.
    host.test_key(tab, 43, ",", Flags::Command, false)?;
    for _ in 0..20 {
        pause(cx).await;
        if view.update_in(cx, |app, _, _| app.settings.is_some())? {
            break;
        }
    }
    ensure!(
        view.update_in(cx, |app, _, _| app.settings.is_some())?,
        "Cmd+, no longer opens settings"
    );
    eprintln!("AXOLOTL_KEYBOARD: settings shortcut complete");
    view.update_in(cx, |app, window, cx| app.close_settings(window, cx))?;
    host.focus(tab)?;
    host.test_key(tab, 37, "l", Flags::Command, false)?;
    for _ in 0..20 {
        pause(cx).await;
        if view.update_in(cx, |app, _, _| app.editing.is_some())? {
            break;
        }
    }
    ensure!(
        view.update_in(cx, |app, _, _| app.editing.is_some())?,
        "Cmd+L no longer opens address editor"
    );
    eprintln!("AXOLOTL_KEYBOARD: address shortcut complete");
    view.update_in(cx, |app, window, cx| {
        app.cancel_edit(window, cx);
        app.begin_edit(
            crate::app::TabTarget::Space(app.model.active_space().id),
            window,
            cx,
        );
    })?;
    pause(cx).await;
    pause(cx).await;
    view.update_in(cx, |app, window, cx| {
        app.address.update(cx, |input, cx| {
            input.focus(window, cx);
            input.set_value("alpha", window, cx);
        });
    })?;
    pause(cx).await;
    host.test_key(
        tab,
        123,
        "\u{f702}",
        Flags::NumericPad | Flags::Function,
        false,
    )?;
    pause(cx).await;
    ensure!(
        view.update_in(cx, |app, _, cx| {
            let input = app.address.read(cx);
            input.value() == "alpha" && input.cursor() == 4
        })?,
        "Arrow routing captured the GPUI shell editor"
    );
    view.update_in(cx, |app, window, cx| {
        app.cancel_edit(window, cx);
        app.focus.focus(window);
    })?;
    host.focus(tab)?;
    let mut failures = vec![];
    for field in ["input", "textarea", "editable"] {
        for (modifier, flags) in [
            ("plain", Flags::empty()),
            ("shift", Flags::Shift),
            ("option", Flags::Option),
            ("option-shift", Flags::Option | Flags::Shift),
            ("command", Flags::Command),
            ("command-shift", Flags::Command | Flags::Shift),
        ] {
            for (code, text, key, plain, option, command) in [
                (123, "\u{f702}", "ArrowLeft", 9, 6, 0),
                (124, "\u{f703}", "ArrowRight", 11, 16, 16),
                (126, "\u{f700}", "ArrowUp", 0, 0, 0),
                (125, "\u{f701}", "ArrowDown", 16, 16, 16),
            ] {
                eval(&host, tab, &format!("prepare('{field}')"), cx).await?;
                host.focus(tab)?;
                press(
                    &host,
                    tab,
                    code,
                    text,
                    flags | Flags::NumericPad | Flags::Function,
                    false,
                    cx,
                )
                .await?;
                let state = eval(&host, tab, "state()", cx).await?;
                let end = if flags.contains(Flags::Command) {
                    command
                } else if flags.contains(Flags::Option) {
                    option
                } else {
                    plain
                };
                let (start, end) = if flags.contains(Flags::Shift) {
                    (end.min(10), end.max(10))
                } else {
                    (end, end)
                };
                let events = state["events"].as_array().unwrap();
                let downs: Vec<_> = events
                    .iter()
                    .filter(|e| e[0] == "keydown" && e[1] == key)
                    .collect();
                if !(state["value"] == "alpha beta gamma"
                    && state["start"] == start
                    && state["end"] == end
                    && downs.len() == 1
                    && downs[0][1] == key
                    && downs[0][2] == true
                    && !events.iter().any(|e| e[0] == "beforeinput"))
                {
                    failures.push(format!(
                        "{field} {modifier} {key}: expected {start}..{end}; {state}"
                    ));
                }
            }
        }
        eprintln!("AXOLOTL_KEYBOARD: {field} arrow matrix complete");
        // Pages must still be able to cancel arrows for their own controls.
        eval(
            &host,
            tab,
            &format!("prepare('{field}'); cancelArrow = true; true"),
            cx,
        )
        .await?;
        press(&host, tab, 123, "\u{f702}", Flags::NumericPad, false, cx).await?;
        let state = eval(&host, tab, "cancelArrow = false; state()", cx).await?;
        ensure!(
            state["start"] == 10 && state["end"] == 10 && state["value"] == "alpha beta gamma",
            "preventDefault ignored: {state}"
        );
        // Ordinary typing after navigation, deletion, and key-repeat metadata.
        eval(&host, tab, &format!("prepare('{field}')"), cx).await?;
        press(&host, tab, 123, "\u{f702}", Flags::empty(), true, cx).await?;
        if !std::env::args().any(|a| a == "--smoke-keyboard-os") {
            ensure!(
                eval(
                    &host,
                    tab,
                    "keyEvents.some(e => e[0] === 'keydown' && e[4])",
                    cx
                )
                .await?
                    == true,
                "repeat flag lost"
            );
        }
        eval(&host, tab, "keyEvents = []; true", cx).await?;
        press(&host, tab, 0, "a", Flags::empty(), false, cx).await?;
        ensure!(
            eval(&host, tab, "state().value", cx).await? == "alpha betaa gamma",
            "normal typing failed in {field}"
        );
        eval(&host, tab, "keyEvents = []; true", cx).await?;
        press(&host, tab, 51, "\u{7f}", Flags::empty(), false, cx).await?;
        ensure!(
            eval(&host, tab, "state().value", cx).await? == "alpha beta gamma",
            "backspace failed in {field}"
        );
    }
    ensure!(
        failures.is_empty(),
        "Native keyboard failures:\n{}",
        failures.join("\n")
    );
    // Vertical movement must follow actual lines, including selection/document navigation.
    for field in ["textarea", "editable"] {
        for (code, text, target) in [(126, "\u{f700}", 5), (125, "\u{f701}", 28)] {
            for shift in [false, true] {
                eval(
                    &host,
                    tab,
                    &format!("prepare('{field}', 'alpha beta\\ngamma delta\\nomega zeta', 16)"),
                    cx,
                )
                .await?;
                let flags = if shift { Flags::Shift } else { Flags::empty() };
                press(&host, tab, code, text, flags, false, cx).await?;
                let state = eval(&host, tab, "state()", cx).await?;
                let (start, end) = if shift {
                    (target.min(16), target.max(16))
                } else {
                    (target, target)
                };
                ensure!(
                    state["start"] == start
                        && state["end"] == end
                        && state["value"] == "alpha beta\ngamma delta\nomega zeta",
                    "multiline {field}: {state}"
                );
            }
        }
    }
    eprintln!("AXOLOTL_KEYBOARD: multiline checks complete");
    Ok(vec![
        "72 native arrow cases: input/textarea/contenteditable, Shift/Option/Command selection and navigation; exactly one trusted keydown and no text input",
        "multiline vertical navigation/selection, preventDefault, repeat, normal typing/backspace, Cmd+, Cmd+L and GPUI shell editing",
    ])
}
