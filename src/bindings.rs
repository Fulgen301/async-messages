#![expect(non_snake_case)]

mod c {
    use nt_user_call::load_runtime_fn;

    load_runtime_fn!(["win32u"] "system" pub fn NtUserGetQueueStatusReadonly(wake_mask_and_flags: u32) -> u32);
}

pub unsafe fn NtUserGetQueueStatusReadonly(
    wake_mask_and_flags: u32,
) -> Result<u32, nt_user_call::error::UserCallError> {
    unsafe {
        c::NtUserGetQueueStatusReadonly(wake_mask_and_flags)
            .or_else(|_| nt_user_call::functions::NtUserGetQueueStatus(wake_mask_and_flags))
    }
}
