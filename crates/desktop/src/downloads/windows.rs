use super::*;
use ::windows::core::{HSTRING, Interface, PWSTR};
use webview2_com::{DownloadStartingEventHandler, Microsoft::Web::WebView2::Win32::*, take_pwstr};
use wry::WebViewExtWindows;

pub struct Transfer {
    operation: ICoreWebView2DownloadOperation,
}
impl Transfer {
    pub fn progress(&self) -> (u64, Option<u64>, Option<std::result::Result<(), String>>) {
        unsafe {
            let mut received = 0;
            let mut total = 0;
            let mut state = COREWEBVIEW2_DOWNLOAD_STATE::default();
            let result = self
                .operation
                .BytesReceived(&mut received)
                .and_then(|()| self.operation.TotalBytesToReceive(&mut total))
                .and_then(|()| self.operation.State(&mut state));
            let outcome = match result {
                Err(error) => Some(Err(format!("Download progress: {error}"))),
                Ok(()) if state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED => Some(Ok(())),
                Ok(()) if state == COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED => {
                    let mut reason = COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON::default();
                    let _ = self.operation.InterruptReason(&mut reason);
                    Some(Err(format!("Download interrupted ({})", reason.0)))
                }
                _ => None,
            };
            (
                received.max(0) as u64,
                (total > 0).then_some(total as u64),
                outcome,
            )
        }
    }
    pub fn cancel(&self) {
        unsafe {
            let _ = self.operation.Cancel();
        }
    }
}

pub fn install(view: &wry::WebView, manager: Rc<RefCell<DownloadManager>>) -> Result<()> {
    let manager = Rc::downgrade(&manager);
    unsafe {
        let webview: ICoreWebView2_4 = view.webview().cast()?;
        webview.add_DownloadStarting(
            &DownloadStartingEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else {
                    return Ok(());
                };
                args.SetHandled(true)?;
                let Some(manager) = manager.upgrade() else {
                    args.SetCancel(true)?;
                    return Ok(());
                };
                let mut suggested = PWSTR::null();
                args.ResultFilePath(&mut suggested)?;
                let suggested = take_pwstr(suggested);
                let result = manager.borrow_mut().begin(&suggested);
                match result {
                    Ok((id, path)) => {
                        let setup = args.DownloadOperation().and_then(|operation| {
                            args.SetResultFilePath(&HSTRING::from(path.as_os_str()))?;
                            Ok(operation)
                        });
                        let mut state = manager.borrow_mut();
                        let transfer = state
                            .transfers
                            .iter_mut()
                            .find(|t| t.item.id == id)
                            .unwrap();
                        match setup {
                            Ok(operation) => transfer.native = Some(Transfer { operation }),
                            Err(error) => {
                                finish(transfer, Err(format!("Start download: {error}")));
                                state.revision += 1;
                                args.SetCancel(true)?;
                            }
                        }
                    }
                    Err(error) => {
                        eprintln!("Download destination: {error}");
                        args.SetCancel(true)?;
                    }
                }
                Ok(())
            })),
            &mut 0,
        )?;
    }
    Ok(())
}

pub fn file_action(path: &Path, reveal: bool) -> Result<()> {
    use ::windows::{
        Win32::UI::{
            Shell::{ILCreateFromPathW, ILFree, SHOpenFolderAndSelectItems, ShellExecuteW},
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
        core::{PCWSTR, w},
    };
    let path = HSTRING::from(path.as_os_str());
    unsafe {
        if reveal {
            let item = ILCreateFromPathW(&path);
            if item.is_null() {
                bail!("Cannot locate downloaded file in Explorer");
            }
            let result = SHOpenFolderAndSelectItems(item, None, 0);
            ILFree(Some(item));
            result.context("Show downloaded file in Explorer")?;
        } else {
            let result = ShellExecuteW(
                None,
                w!("open"),
                &path,
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            );
            if result.0 as isize <= 32 {
                bail!(
                    "Cannot open downloaded file (Windows error {})",
                    result.0 as isize
                );
            }
        }
    }
    Ok(())
}

/// CopyFile preserves the native engine's Windows security metadata and streams.
pub fn copy_new_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    use ::windows::Win32::Storage::FileSystem::CopyFileW;
    unsafe {
        CopyFileW(
            &HSTRING::from(source.as_os_str()),
            &HSTRING::from(destination.as_os_str()),
            true,
        )
        .map_err(|error| std::io::Error::from_raw_os_error((error.code().0 as u32 & 0xffff) as i32))
    }
}
