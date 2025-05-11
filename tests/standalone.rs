use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use async_messages::*;
use tokio::task::LocalSet;
use windows::{
    Win32::{
        Foundation::{LPARAM, WAIT_OBJECT_0, WPARAM},
        System::Threading::{CreateEventW, GetCurrentThreadId, WaitForSingleObject},
        UI::WindowsAndMessaging::{
            MSG, MWMO_INPUTAVAILABLE, MWMO_NONE, MsgWaitForMultipleObjects, PM_NOREMOVE, PM_REMOVE,
            PeekMessageW, PostThreadMessageW, QS_ALLPOSTMESSAGE, QS_MOUSEBUTTON, WM_USER,
        },
    },
    core::Owned,
};

mod handle_waker {
    use std::task::Waker;

    use windows::Win32::Foundation::HANDLE;

    mod vtable {
        use std::{
            mem::MaybeUninit,
            task::{RawWaker, RawWakerVTable},
        };

        use windows::Win32::{
            Foundation::{CloseHandle, DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE},
            System::Threading::{GetCurrentProcess, SetEvent},
        };

        pub fn from_handle(handle: HANDLE) -> windows::core::Result<RawWaker> {
            let new_handle = unsafe {
                let mut new_handle = MaybeUninit::uninit();

                DuplicateHandle(
                    GetCurrentProcess(),
                    handle,
                    GetCurrentProcess(),
                    new_handle.as_mut_ptr(),
                    0,
                    false,
                    DUPLICATE_SAME_ACCESS,
                )?;

                new_handle.assume_init()
            };

            Ok(RawWaker::new(new_handle.0 as _, &VTABLE))
        }

        fn clone(data: *const ()) -> RawWaker {
            from_handle(HANDLE(data as _)).unwrap()
        }

        fn wake(data: *const ()) {
            let handle = HANDLE(data as _);
            unsafe {
                SetEvent(handle).unwrap();
                CloseHandle(handle).unwrap();
            }
        }

        fn wake_by_ref(data: *const ()) {
            let handle = HANDLE(data as _);
            unsafe {
                SetEvent(handle).unwrap();
            }
        }

        fn drop(_data: *const ()) {
            unsafe {
                CloseHandle(HANDLE(_data as _)).unwrap();
            }
        }

        pub static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
    }

    pub fn handle_waker(handle: HANDLE) -> windows::core::Result<Waker> {
        unsafe { Ok(Waker::from_raw(vtable::from_handle(handle)?)) }
    }
}

fn in_new_thread(f: impl FnOnce() + Send + 'static) {
    std::thread::spawn(f).join().unwrap();
}

fn in_new_thread_local_set(f: impl FnOnce() + Send + 'static) {
    in_new_thread(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let local = LocalSet::new();
        let _guard = local.enter();
        f();

        runtime.block_on(local);
    });
}

#[test]
pub fn thread_messages() {
    in_new_thread(|| unsafe {
        let mut msg = MSG::default();
        assert!(!PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE).as_bool());

        let mut future = wait_for_messages(QS_ALLPOSTMESSAGE, MWMO_NONE).unwrap();

        let event = Owned::new(CreateEventW(None, true, false, None).unwrap());

        let waker = handle_waker::handle_waker(*event).unwrap();
        let mut context = Context::from_waker(&waker);

        assert!(matches!(
            Pin::new_unchecked(&mut future).poll(&mut context),
            Poll::Pending
        ));

        PostThreadMessageW(GetCurrentThreadId(), WM_USER, WPARAM(0), LPARAM(0)).unwrap();

        assert_eq!(WaitForSingleObject(*event, 2000), WAIT_OBJECT_0);
    });
}

#[link(name = "win32u", kind = "raw-dylib")]
unsafe extern "system" {
    pub unsafe fn NtUserGetQueueStatusReadonly(wake_mask_and_flags: u32) -> u32;
}

#[test]
pub fn test_messages_local_set() {
    in_new_thread_local_set(|| {
        tokio::task::spawn_local(async {
            let mut msg = MSG::default();
            unsafe {
                assert!(!PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE).as_bool());
            }

            let future =
                Box::pin(wait_for_messages(QS_ALLPOSTMESSAGE, MWMO_INPUTAVAILABLE).unwrap());

            unsafe {
                let mut msg = MSG::default();

                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {}
            }

            let status = unsafe {
                NtUserGetQueueStatusReadonly((QS_ALLPOSTMESSAGE.0 << 16) | QS_ALLPOSTMESSAGE.0)
            };

            assert_eq!(status, 0);

            unsafe {
                PostThreadMessageW(GetCurrentThreadId(), WM_USER, WPARAM(0), LPARAM(0)).unwrap();
            }

            let status = unsafe {
                NtUserGetQueueStatusReadonly((QS_ALLPOSTMESSAGE.0 << 16) | QS_ALLPOSTMESSAGE.0)
            };

            assert_ne!(status, 0);

            tokio::task::spawn_local(future);
        });
    });
}

#[test]
fn queue_attach() {
    in_new_thread(|| unsafe {
        let mut msg = MSG::default();
        assert!(!PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE).as_bool());

        let mut future = wait_for_messages(QS_ALLPOSTMESSAGE, MWMO_INPUTAVAILABLE).unwrap();

        let event = Owned::new(CreateEventW(None, true, false, None).unwrap());

        let waker = handle_waker::handle_waker(*event).unwrap();
        let mut context = Context::from_waker(&waker);

        assert!(matches!(
            Pin::new_unchecked(&mut future).poll(&mut context),
            Poll::Pending
        ));

        _ = MsgWaitForMultipleObjects(Some(&[*event]), false, 1, QS_MOUSEBUTTON);

        PostThreadMessageW(GetCurrentThreadId(), WM_USER, WPARAM(0), LPARAM(0)).unwrap();

        assert_eq!(WaitForSingleObject(*event, 2000), WAIT_OBJECT_0);
    });
}
