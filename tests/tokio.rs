mod helpers;

use async_messages::wait_for_messages;
use tokio::{
    runtime::Builder,
    sync::oneshot::{self, Sender},
    task::LocalSet,
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    UI::WindowsAndMessaging::{
        DefWindowProcW, DispatchMessageW, HWND_MESSAGE, MWMO_INPUTAVAILABLE, PostMessageW,
        PostQuitMessage, QS_ALLINPUT, SetTimer, TranslateMessage, WM_CLOSE, WM_NULL, WM_QUIT,
        WM_TIMER,
    },
};

use helpers::window::{create_window, register_window_class};

const TIMER_EVENT_ID: usize = 1000;

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if msg == WM_CLOSE || (msg == WM_TIMER && wparam.0 == TIMER_EVENT_ID) {
            PostQuitMessage(0);
        }

        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

#[test]
fn local_set_and_channel() {
    let runtime = Builder::new_current_thread().build().unwrap();

    let window_class = register_window_class(Some(window_proc)).unwrap();
    let window = create_window(&window_class, Some(HWND_MESSAGE)).unwrap();

    unsafe {
        SetTimer(Some(**window), TIMER_EVENT_ID, 500, None);
        PostMessageW(Some(**window), WM_NULL, WPARAM(0), LPARAM(0)).unwrap();
    }

    let (tx, rx) = oneshot::channel::<()>();

    async fn mainloop(tx: Sender<()>) -> windows::core::Result<()> {
        loop {
            for msg in wait_for_messages(QS_ALLINPUT, MWMO_INPUTAVAILABLE)?.await? {
                if msg.message == WM_QUIT {
                    _ = tx.send(());
                    return Ok(());
                }

                unsafe {
                    _ = TranslateMessage(&raw const msg);
                    DispatchMessageW(&raw const msg);
                }
            }
        }
    }

    let local = LocalSet::new();
    local.block_on(&runtime, async move {
        tokio::task::spawn_local(mainloop(tx));
        tokio::task::spawn_local(async move {
            rx.await.unwrap();
        });
    });

    runtime.block_on(local);
}
