use std::ops::Deref;

use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        UI::WindowsAndMessaging::{
            CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, IDC_ARROW, LoadCursorW,
            PostQuitMessage, RegisterClassExW, UnregisterClassW, WINDOW_EX_STYLE, WM_CLOSE,
            WNDCLASS_STYLES, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
        },
    },
    core::{Owned, PCWSTR, w},
};

use crate::helpers::get_instance_handle;

#[repr(transparent)]
pub struct OwnedHWND {
    hwnd: HWND,
}

impl Deref for OwnedHWND {
    type Target = HWND;

    fn deref(&self) -> &Self::Target {
        &self.hwnd
    }
}

impl windows::core::Free for OwnedHWND {
    unsafe fn free(&mut self) {
        if self.hwnd != HWND::default() {
            DestroyWindow(self.hwnd).unwrap();
        }
    }
}

#[repr(transparent)]
pub struct WindowClass(pub u16);

impl WindowClass {
    pub fn as_pcwstr(&self) -> PCWSTR {
        PCWSTR(self.0 as _)
    }
}

impl Deref for WindowClass {
    type Target = u16;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<WindowClass> for PCWSTR {
    fn from(value: WindowClass) -> Self {
        PCWSTR(value.0 as _)
    }
}

impl windows::core::Free for WindowClass {
    unsafe fn free(&mut self) {
        if self.0 != 0 {
            let _ = UnregisterClassW(PCWSTR(self.0 as _), None);
        }
    }
}

unsafe extern "system" fn default_window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_CLOSE {
        PostQuitMessage(0);
    }

    DefWindowProcW(hwnd, msg, wparam, lparam)
}

pub fn register_window_class(
    wndproc: Option<unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT>,
) -> windows::core::Result<Owned<WindowClass>> {
    const CLASS_NAME: PCWSTR = w!("ASYNCMESSAGESTEST");

    let hinstance = unsafe { get_instance_handle(PCWSTR::null())? };

    let wndclassex = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as _,
        style: WNDCLASS_STYLES(0),
        lpfnWndProc: wndproc.or(Some(default_window_proc)),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: unsafe { get_instance_handle(PCWSTR::null())? },
        hIcon: Default::default(),
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
        hbrBackground: Default::default(),
        lpszMenuName: PCWSTR::null(),
        lpszClassName: CLASS_NAME,
        hIconSm: Default::default(),
    };

    unsafe {
        _ = UnregisterClassW(CLASS_NAME, Some(hinstance));
    }

    let class_atom = unsafe { RegisterClassExW(&wndclassex) };
    if class_atom != 0 {
        Ok(unsafe { Owned::new(WindowClass(class_atom)) })
    } else {
        Err(windows::core::Error::from_win32())
    }
}

pub fn create_window(
    window_class: &WindowClass,
    parent: Option<HWND>,
) -> windows::core::Result<Owned<OwnedHWND>> {
    unsafe {
        Ok(Owned::new(OwnedHWND {
            hwnd: CreateWindowExW(
                WINDOW_EX_STYLE(0),
                window_class.as_pcwstr(),
                w!("msg_future_test"),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                parent,
                None,
                None,
                None,
            )?,
        }))
    }
}
