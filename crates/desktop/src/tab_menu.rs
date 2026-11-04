//! Modal menu surface above native pages; the transparent backdrop dismisses
//! without forwarding the same click into a page. Page views stay untouched.
use std::sync::mpsc::{self, Receiver};

use anyhow::Result;
use gpui::{Bounds, Pixels, Point, Size, Window, point, px};
use wry::{NewWindowResponse, WebView, WebViewBuilder};

use crate::app::TabAction;
use workspace_core::Axis;

pub enum MenuEvent {
    Action(TabAction),
    Dismiss,
}

pub struct NativeTabMenu {
    view: WebView,
    events: Receiver<MenuEvent>,
}

impl NativeTabMenu {
    pub fn new(position: Point<Pixels>, space: bool, window: &Window) -> Result<Self> {
        let (sender, events) = mpsc::sync_channel(16);
        let sibling = if space {
            "New sibling space"
        } else {
            "New sibling tab"
        };
        let close = if space { "Close space" } else { "Close tab" };
        let x = f32::from(position.x);
        let y = f32::from(position.y);
        let view = WebViewBuilder::new()
            .with_transparent(true)
            .with_navigation_handler(|url| url == "about:blank")
            .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
            .with_ipc_handler(move |request| {
                let event = match request.body().as_str() {
                    "new" => MenuEvent::Action(TabAction::NewSibling),
                    "horizontal" => MenuEvent::Action(TabAction::Split(Axis::Horizontal)),
                    "vertical" => MenuEvent::Action(TabAction::Split(Axis::Vertical)),
                    "close" => MenuEvent::Action(TabAction::Close),
                    "dismiss" => MenuEvent::Dismiss,
                    _ => return,
                };
                let _ = sender.try_send(event);
            })
            .with_html(format!(r#"<!doctype html><meta charset="utf-8">
<style>
*{{box-sizing:border-box}}html,body{{margin:0;width:100%;height:100%;background:transparent}}
.menu{{position:absolute;left:clamp(4px,{x}px,calc(100vw - 232px));top:clamp(38px,{y}px,calc(100vh - 146px));width:224px;padding:4px;border:1px solid #465366;border-radius:8px;background:#25303d;box-shadow:0 12px 28px #0004}}
button{{display:block;width:100%;height:32px;padding:0 8px;border:0;border-radius:4px;background:transparent;color:#dde5ed;text-align:left;font:12px -apple-system,Segoe UI,sans-serif;cursor:pointer}}
button:hover,button:focus-visible{{background:#3a4859;outline:none}}
</style><div class="menu" role="menu"><button role="menuitem" data-action="new">{sibling}</button><button role="menuitem" data-action="horizontal">Split horizontally  ↔</button><button role="menuitem" data-action="vertical">Split vertically  ↕</button><button role="menuitem" data-action="close">{close}</button></div>
<script>
const send=window.ipc.postMessage.bind(window.ipc);
document.addEventListener('pointerdown',event=>{{if(event.isTrusted&&!event.target.closest('.menu'))send('dismiss')}});
document.addEventListener('click',event=>{{const button=event.target.closest('button');if(event.isTrusted&&button)send(button.dataset.action)}});
document.addEventListener('keydown',event=>{{
 if(!event.isTrusted)return;
 if(event.key==='Escape'){{event.preventDefault();send('dismiss')}}
 if(event.key==='ArrowDown'||event.key==='ArrowUp'){{event.preventDefault();const items=[...document.querySelectorAll('button')];const index=items.indexOf(document.activeElement);items[(index+(event.key==='ArrowDown'?1:items.length-1)+items.length)%items.length].focus()}}
}});
document.querySelector('button').focus();
</script>"#))
            .with_bounds(crate::browser::native_bounds(Bounds {
                origin: point(px(0.), px(0.)),
                size: window.viewport_size(),
            }))
            .build_as_child(window)?;
        view.focus()?;
        Ok(Self { view, events })
    }

    pub fn place(&self, viewport: Size<Pixels>) -> Result<()> {
        self.view.set_bounds(crate::browser::native_bounds(Bounds {
            origin: point(px(0.), px(0.)),
            size: viewport,
        }))?;
        Ok(())
    }

    pub fn next_event(&self) -> Option<MenuEvent> {
        self.events.try_recv().ok()
    }
}

impl Drop for NativeTabMenu {
    fn drop(&mut self) {
        let _ = self.view.set_visible(false);
    }
}
