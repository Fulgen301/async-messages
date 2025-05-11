pub mod window;

use windows::{
    Win32::{Foundation::HINSTANCE, System::LibraryLoader::GetModuleHandleW},
    core::{PCWSTR, Param},
};

#[inline]
pub unsafe fn get_instance_handle(
    lpmodulename: impl Param<PCWSTR>,
) -> windows::core::Result<HINSTANCE> {
    GetModuleHandleW(lpmodulename).map(|module| HINSTANCE(module.0))
}
