mod helpers;

use std::future::Future;

use async_messages::wait_for_messages;
use futures_testing::{Driver, TestCase, drive_fn};
use helpers::window::{create_window, register_window_class};
use windows::Win32::{
    Foundation::{LPARAM, WPARAM},
    UI::WindowsAndMessaging::{HWND_MESSAGE, MWMO_NONE, PostMessageW, QS_ALLEVENTS, WM_USER},
};

struct MessageFutureTestCase;

impl<'b> TestCase<'b> for MessageFutureTestCase {
    type Args = ();

    fn init<'a>(&self, _args: &'a mut Self::Args) -> (impl Driver<'b>, impl Future) {
        let window_class = register_window_class(None).unwrap();
        let window = create_window(&window_class, Some(HWND_MESSAGE)).unwrap();

        let driver = drive_fn(move |()| unsafe {
            PostMessageW(Some(**window), WM_USER, WPARAM(0), LPARAM(0)).unwrap()
        });

        let future = async move { wait_for_messages(QS_ALLEVENTS, MWMO_NONE).unwrap().await };

        (driver, future)
    }
}

//#[test]
pub fn future_waker_works_properly() {
    futures_testing::tests(MessageFutureTestCase).run();
}
