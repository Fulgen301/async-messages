use std::{
    future::Future,
    marker::{PhantomData, PhantomPinned},
    mem::MaybeUninit,
    pin::Pin,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
    task::{Context, Poll, Waker},
};

use helpers::ConfiguredInputEvent;
use nt_user_call::functions::NtUserSetWaitForQueueAttach;
use windows::{
    Win32::{
        Foundation::E_INVALIDARG,
        System::Threading::{
            CreateThreadpoolWait, PTP_CALLBACK_INSTANCE, PTP_WAIT, SetThreadpoolWait,
            SetThreadpoolWaitEx, WaitForThreadpoolWaitCallbacks,
        },
        UI::WindowsAndMessaging::{
            MSG, MSG_WAIT_FOR_MULTIPLE_OBJECTS_EX_FLAGS, MWMO_ALERTABLE, MWMO_WAITALL, PM_REMOVE,
            PeekMessageW, QUEUE_STATUS_FLAGS,
        },
    },
    core::Owned,
};

use crate::bindings::NtUserGetQueueStatusReadonly;

pub const MWMO_QUEUEATTACH: MSG_WAIT_FOR_MULTIPLE_OBJECTS_EX_FLAGS =
    MSG_WAIT_FOR_MULTIPLE_OBJECTS_EX_FLAGS(0x0008);

const fn make_dword(low: u16, high: u16) -> u32 {
    (low as u32) | ((high as u32) << 16)
}

mod helpers {
    use std::{ffi::c_void, ptr::NonNull};

    use nt_user_call::functions::{
        NtUserCancelQueueEventCompletionPacket, NtUserClearWakeMask, NtUserGetInputEvent,
        NtUserReassociateQueueEventCompletionPacket,
    };
    use windows::Win32::Foundation::HANDLE;

    use super::make_dword;

    /// Wraps the thread's input event and configures it so that it can be waited on.
    pub struct ConfiguredInputEvent {
        input_event: NonNull<c_void>,
    }

    impl ConfiguredInputEvent {
        pub fn new(queue_status_flags: u16, wait_flags: u16) -> windows::core::Result<Self> {
            let input_event =
                unsafe { NtUserGetInputEvent(make_dword(queue_status_flags, wait_flags))? };

            // Windows 10 introduced an I/O completion port into the message queue. This has the side effect that out
            // If the input event is associated with its wait completion packet, our wait won't get properly woken up.
            // To work around this, we do what MsgWaitForMultipleObjectsEx does when it waits for all events:
            // Cancel the wait completion packet and reassociate it when the wait is done.
            // We don't care about the result here - if the call isn't found, the OS doesn't have it, and the system call
            // itself does not return any information as to whether cancellation succeeded or not.
            unsafe {
                _ = NtUserCancelQueueEventCompletionPacket();
            }

            Ok(Self {
                // SAFETY: `input_event` has been checked above
                input_event: unsafe { NonNull::new_unchecked(input_event.0) },
            })
        }

        pub fn as_raw(&self) -> HANDLE {
            HANDLE(self.input_event.as_ptr())
        }
    }

    impl Drop for ConfiguredInputEvent {
        fn drop(&mut self) {
            // The order of the calls matches MsgWaitForMultipleObjectsEx.
            unsafe {
                NtUserClearWakeMask().unwrap();
                _ = NtUserReassociateQueueEventCompletionPacket();
            }
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug)]
enum InputEventFutureState {
    NotPending,
    Pending,
    Ready,
    Cancelled,
}

struct InputEventFutureShared {
    state: AtomicU32,
    waker_in_use: AtomicBool,
    waker: Option<Waker>,
}

impl InputEventFutureShared {
    pub fn wait_done(&self) {
        let old_state = self
            .state
            .swap(InputEventFutureState::Ready as _, Ordering::AcqRel);

        // If old_state is NotPending, there is nothing to wake as poll() will immediately return Ready.
        // If old_state is Cancelled, the future is being dropped and there is no need to wake the waker
        if old_state != InputEventFutureState::Pending as u32 {
            return;
        }

        while self
            .waker_in_use
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {}
        self.waker.as_ref().unwrap().wake_by_ref();
        self.waker_in_use.store(false, Ordering::Release);
    }
}

impl Default for InputEventFutureShared {
    fn default() -> Self {
        Self {
            state: AtomicU32::new(InputEventFutureState::NotPending as _),
            waker_in_use: AtomicBool::new(false),
            waker: None,
        }
    }
}

struct InputEventFuture {
    queue_status_flags: u16,
    wait_flags: u16,
    input_event: Option<ConfiguredInputEvent>,
    shared: InputEventFutureShared,
    ptp_wait: Owned<PTP_WAIT>,
    _marker: PhantomPinned,
}

impl InputEventFuture {
    pub fn new(queue_status_flags: u16, wait_flags: u16) -> Self {
        Self {
            queue_status_flags,
            wait_flags,
            input_event: None,
            shared: InputEventFutureShared::default(),
            ptp_wait: Owned::default(),
            _marker: PhantomPinned,
        }
    }

    fn ready(self: Pin<&mut Self>) -> Poll<<Self as Future>::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        std::mem::drop(this.input_event.take());
        std::mem::drop(std::mem::take(&mut this.ptp_wait));

        Poll::Ready(Ok(MessageIterator::default()))
    }

    unsafe extern "system" fn callback(
        _instance: PTP_CALLBACK_INSTANCE,
        context: *mut core::ffi::c_void,
        _wait: PTP_WAIT,
        _waitresult: u32,
    ) {
        let this = unsafe { &*(context as *const InputEventFutureShared) };
        this.wait_done();
    }
}

impl Drop for InputEventFuture {
    fn drop(&mut self) {
        if self
            .shared
            .state
            .compare_exchange(
                InputEventFutureState::Pending as _,
                InputEventFutureState::Cancelled as _,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            unsafe {
                if !SetThreadpoolWaitEx(*self.ptp_wait, None, None, None).as_bool() {
                    WaitForThreadpoolWaitCallbacks(*self.ptp_wait, true);
                }
            }
        }
    }
}

impl Future for InputEventFuture {
    type Output = windows::core::Result<MessageIterator>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let state = self.shared.state.load(Ordering::Acquire);
        if state == InputEventFutureState::Ready as u32 {
            return self.ready();
        } else if state == InputEventFutureState::Pending as u32 {
            match self.shared.waker_in_use.compare_exchange(
                false,
                true,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    unsafe {
                        let this = self.get_unchecked_mut();
                        this.shared.waker = Some(cx.waker().clone());
                        this.shared.waker_in_use.store(false, Ordering::Release);
                    }
                    return Poll::Pending;
                }
                Err(_) => {
                    // The callback is currently using the old waker - no need to replace it, we'll be ready soon
                    return Poll::Pending;
                }
            }
        }

        let queue_status = unsafe {
            NtUserGetQueueStatusReadonly(make_dword(self.queue_status_flags, self.wait_flags))
        }?;

        // Messages are already in the queue
        if queue_status > 0 {
            return Poll::Ready(Ok(MessageIterator::default()));
        }

        let wait = unsafe {
            Owned::new(CreateThreadpoolWait(
                Some(Self::callback),
                Some({
                    let this = self.as_mut().get_unchecked_mut();
                    &raw mut this.shared as _
                }),
                None,
            )?)
        };
        unsafe {
            let this = self.as_mut().get_unchecked_mut();

            this.input_event = Some(ConfiguredInputEvent::new(
                this.queue_status_flags,
                this.wait_flags,
            )?);
        }

        if self.queue_status_flags & (MWMO_QUEUEATTACH.0 as u16) != 0 {
            unsafe {
                _ = NtUserSetWaitForQueueAttach(true.into())?;
            }
        }

        unsafe {
            let this = self.as_mut().get_unchecked_mut();
            this.shared.waker = Some(cx.waker().clone());
            this.ptp_wait = wait;
        }

        unsafe {
            SetThreadpoolWait(
                *self.ptp_wait,
                Some(self.input_event.as_ref().unwrap().as_raw()),
                None,
            );
        }

        match self.shared.state.compare_exchange(
            InputEventFutureState::NotPending as _,
            InputEventFutureState::Pending as _,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => Poll::Pending,
            Err(_) => {
                // The wait already finished in the meantime.
                self.ready()
            }
        }
    }
}

pub fn wait_for_messages(
    queue_status_flags: QUEUE_STATUS_FLAGS,
    wait_flags: MSG_WAIT_FOR_MULTIPLE_OBJECTS_EX_FLAGS,
) -> windows::core::Result<impl Future<Output = windows::core::Result<impl Iterator<Item = MSG>>>> {
    if wait_flags.0 & (MWMO_ALERTABLE.0 | MWMO_WAITALL.0) != 0 {
        return Err(E_INVALIDARG.into());
    }

    let queue_status_flags = queue_status_flags.0.try_into().map_err(|_| E_INVALIDARG)?;
    let wait_flags = wait_flags.0.try_into().map_err(|_| E_INVALIDARG)?;

    Ok(InputEventFuture::new(queue_status_flags, wait_flags))
}

struct MessageIterator {
    _marker: PhantomData<*mut ()>,
}

impl Default for MessageIterator {
    fn default() -> Self {
        MessageIterator {
            _marker: PhantomData,
        }
    }
}

impl Iterator for MessageIterator {
    type Item = MSG;

    fn next(&mut self) -> Option<Self::Item> {
        let mut msg = MaybeUninit::uninit();
        if unsafe { PeekMessageW(msg.as_mut_ptr(), None, 0, 0, PM_REMOVE).as_bool() } {
            Some(unsafe { msg.assume_init() })
        } else {
            None
        }
    }
}
