//! AAPCS boundaries for worker kernel calls.
//!
//! A raw SVC may overwrite caller-saved registers. Keep it out of Rust inline
//! assembly so LLVM treats r0-r3/r12 and flags as volatile across each call.
//! Register arguments follow devkitPro/libctru source/svc.s and include/3ds/svc.h.

#[cfg(any(target_arch = "arm", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CapacityError {
    Native(u32),
    Unavailable,
}

#[cfg(any(target_arch = "arm", test))]
trait ResourceApi {
    fn acquire(&mut self, handle: &mut u32) -> i32;
    fn limit(&mut self, handle: u32, value: &mut i64) -> i32;
    fn current(&mut self, handle: u32, value: &mut i64) -> i32;
    fn close(&mut self, handle: u32) -> i32;
}

#[cfg(any(target_arch = "arm", test))]
fn native_result(code: i32) -> Result<(), CapacityError> {
    if code < 0 {
        Err(CapacityError::Native(code as u32))
    } else {
        Ok(())
    }
}

#[cfg(any(target_arch = "arm", test))]
fn check_capacity(api: &mut impl ResourceApi, additional: i64) -> Result<(), CapacityError> {
    if additional < 0 {
        return Err(CapacityError::Unavailable);
    }
    let mut handle = 0;
    native_result(api.acquire(&mut handle))?;
    if handle == 0 {
        return Err(CapacityError::Unavailable);
    }
    let queried = (|| {
        let mut limit = 0;
        native_result(api.limit(handle, &mut limit))?;
        let mut current = 0;
        native_result(api.current(handle, &mut current))?;
        if current < 0
            || limit < 0
            || current
                .checked_add(additional)
                .is_none_or(|needed| needed > limit)
        {
            return Err(CapacityError::Unavailable);
        }
        Ok(())
    })();
    // Own exactly one real handle after acquire. Always attempt its close,
    // including failed queries, and retain the first error if close also fails.
    let closed = native_result(api.close(handle));
    queried.and(closed)
}

#[cfg(target_arch = "arm")]
mod arm {
    use super::*;
    use core::arch::global_asm;

    global_asm!(
        ".syntax unified",
        ".arm",
        ".section .text.bank.kernel, \"ax\", %progbits",
        ".align 2",
        ".global bank_svc_close_handle",
        ".type bank_svc_close_handle, %function",
        "bank_svc_close_handle:",
        "push {{r4, lr}}",
        "svc #0x23",
        "pop {{r4, pc}}",
        ".size bank_svc_close_handle, .-bank_svc_close_handle",
        ".global bank_svc_wait_thread",
        ".type bank_svc_wait_thread, %function",
        "bank_svc_wait_thread:",
        "push {{r4, lr}}",
        // AAPCS aligns the i64 timeout in r2/r3; r1 is unused.
        "svc #0x24",
        "pop {{r4, pc}}",
        ".size bank_svc_wait_thread, .-bank_svc_wait_thread",
        ".global bank_svc_get_resource_limit",
        ".type bank_svc_get_resource_limit, %function",
        "bank_svc_get_resource_limit:",
        // SVC38 takes the process in r1, then returns Result in r0 and the
        // acquired handle in r1. Preserve the caller's out-pointer on stack.
        "push {{r0, lr}}",
        "svc #0x38",
        "ldr r2, [sp]",
        "str r1, [r2]",
        "add sp, sp, #4",
        "pop {{pc}}",
        ".size bank_svc_get_resource_limit, .-bank_svc_get_resource_limit",
        ".global bank_svc_get_resource_limit_values",
        ".type bank_svc_get_resource_limit_values, %function",
        "bank_svc_get_resource_limit_values:",
        "push {{r4, lr}}",
        // r0=values*, r1=resource handle, r2=names*, r3=name count.
        "svc #0x39",
        "pop {{r4, pc}}",
        ".size bank_svc_get_resource_limit_values, .-bank_svc_get_resource_limit_values",
        ".global bank_svc_get_resource_current_values",
        ".type bank_svc_get_resource_current_values, %function",
        "bank_svc_get_resource_current_values:",
        "push {{r4, lr}}",
        "svc #0x3a",
        "pop {{r4, pc}}",
        ".size bank_svc_get_resource_current_values, .-bank_svc_get_resource_current_values",
    );

    unsafe extern "aapcs" {
        /// Closes the supplied real native kernel handle.
        #[link_name = "bank_svc_close_handle"]
        pub(crate) fn close_handle(handle: u32) -> i32;
        /// Poll with timeout0; a positive timeout may block the calling thread.
        #[link_name = "bank_svc_wait_thread"]
        pub(crate) fn wait_thread(handle: u32, timeout: i64) -> i32;
        #[link_name = "bank_svc_get_resource_limit"]
        fn get_resource_limit(output: *mut u32, process: u32) -> i32;
        #[link_name = "bank_svc_get_resource_limit_values"]
        fn get_resource_limit_values(
            values: *mut i64,
            resource: u32,
            names: *const u32,
            count: i32,
        ) -> i32;
        #[link_name = "bank_svc_get_resource_current_values"]
        fn get_resource_current_values(
            values: *mut i64,
            resource: u32,
            names: *const u32,
            count: i32,
        ) -> i32;
    }

    struct Kernel;
    impl ResourceApi for Kernel {
        fn acquire(&mut self, handle: &mut u32) -> i32 {
            unsafe { get_resource_limit(handle, 0xffff_8001) }
        }
        fn limit(&mut self, handle: u32, value: &mut i64) -> i32 {
            let name = 2u32; // RESLIMIT_THREAD, never a resource-limit mutation.
            unsafe { get_resource_limit_values(value, handle, &name, 1) }
        }
        fn current(&mut self, handle: u32, value: &mut i64) -> i32 {
            let name = 2u32;
            unsafe { get_resource_current_values(value, handle, &name, 1) }
        }
        fn close(&mut self, handle: u32) -> i32 {
            unsafe { close_handle(handle) }
        }
    }

    pub(crate) fn check_thread_capacity(additional: i64) -> Result<(), CapacityError> {
        check_capacity(&mut Kernel, additional)
    }
}

#[cfg(target_arch = "arm")]
pub(crate) use arm::{check_thread_capacity, close_handle, wait_thread};

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{vec, vec::Vec};

    const HANDLE: u32 = 0x8765_4321;
    const BAD_HANDLE: i32 = 0xd8e0_07f7u32 as i32;
    const QUERY_FAILURE: i32 = 0xd900_1830u32 as i32;
    const CLOSE_FAILURE: i32 = 0xd8a0_07fau32 as i32;

    #[derive(Debug, PartialEq, Eq)]
    enum Call {
        Acquire,
        Limit(u32),
        Current(u32),
        Close(u32),
    }

    struct Mock {
        acquired: (i32, u32),
        limit: (i32, i64),
        current: (i32, i64),
        close_result: i32,
        calls: Vec<Call>,
    }
    impl Mock {
        fn healthy(limit: i64, current: i64) -> Self {
            Self {
                acquired: (0, HANDLE),
                limit: (0, limit),
                current: (0, current),
                close_result: 0,
                calls: Vec::new(),
            }
        }
    }
    impl ResourceApi for Mock {
        fn acquire(&mut self, handle: &mut u32) -> i32 {
            self.calls.push(Call::Acquire);
            *handle = self.acquired.1;
            self.acquired.0
        }
        fn limit(&mut self, handle: u32, value: &mut i64) -> i32 {
            self.calls.push(Call::Limit(handle));
            *value = self.limit.1;
            self.limit.0
        }
        fn current(&mut self, handle: u32, value: &mut i64) -> i32 {
            self.calls.push(Call::Current(handle));
            *value = self.current.1;
            self.current.0
        }
        fn close(&mut self, handle: u32) -> i32 {
            self.calls.push(Call::Close(handle));
            self.close_result
        }
    }
    fn all_calls() -> Vec<Call> {
        vec![
            Call::Acquire,
            Call::Limit(HANDLE),
            Call::Current(HANDLE),
            Call::Close(HANDLE),
        ]
    }

    #[test]
    fn acquired_handle_is_preserved_and_closed_exactly_once() {
        let mut api = Mock::healthy(32, 8);
        assert_eq!(check_capacity(&mut api, 2), Ok(()));
        assert_eq!(api.calls, all_calls());
    }

    #[test]
    fn acquisition_failure_never_queries_or_closes_undefined_output() {
        let mut api = Mock::healthy(32, 8);
        api.acquired = (BAD_HANDLE, HANDLE);
        assert_eq!(
            check_capacity(&mut api, 2),
            Err(CapacityError::Native(BAD_HANDLE as u32))
        );
        assert_eq!(api.calls, vec![Call::Acquire]);
    }

    #[test]
    fn successful_acquisition_without_handle_is_rejected() {
        let mut api = Mock::healthy(32, 8);
        api.acquired.1 = 0;
        assert_eq!(check_capacity(&mut api, 2), Err(CapacityError::Unavailable));
        assert_eq!(api.calls, vec![Call::Acquire]);
    }

    #[test]
    fn limit_failure_closes_handle_and_preserves_first_error() {
        let mut api = Mock::healthy(32, 8);
        api.limit.0 = QUERY_FAILURE;
        api.close_result = CLOSE_FAILURE;
        assert_eq!(
            check_capacity(&mut api, 2),
            Err(CapacityError::Native(QUERY_FAILURE as u32))
        );
        assert_eq!(
            api.calls,
            vec![Call::Acquire, Call::Limit(HANDLE), Call::Close(HANDLE)]
        );
    }

    #[test]
    fn current_failure_closes_handle_and_preserves_first_error() {
        let mut api = Mock::healthy(32, 8);
        api.current.0 = QUERY_FAILURE;
        api.close_result = CLOSE_FAILURE;
        assert_eq!(
            check_capacity(&mut api, 2),
            Err(CapacityError::Native(QUERY_FAILURE as u32))
        );
        assert_eq!(api.calls, all_calls());
    }

    #[test]
    fn close_failure_is_reported_after_successful_queries() {
        let mut api = Mock::healthy(32, 8);
        api.close_result = CLOSE_FAILURE;
        assert_eq!(
            check_capacity(&mut api, 2),
            Err(CapacityError::Native(CLOSE_FAILURE as u32))
        );
        assert_eq!(api.calls, all_calls());
    }

    #[test]
    fn exact_capacity_is_allowed_and_excess_is_rejected_after_close() {
        for (additional, expected) in [(2, Ok(())), (3, Err(CapacityError::Unavailable))] {
            let mut api = Mock::healthy(10, 8);
            assert_eq!(check_capacity(&mut api, additional), expected);
            assert_eq!(api.calls, all_calls());
        }
    }

    #[test]
    fn negative_values_and_addition_overflow_are_rejected() {
        for (limit, current, additional) in [(-1, 0, 1), (32, -1, 1), (i64::MAX, i64::MAX, 1)] {
            let mut api = Mock::healthy(limit, current);
            assert_eq!(
                check_capacity(&mut api, additional),
                Err(CapacityError::Unavailable)
            );
            assert_eq!(api.calls, all_calls());
        }
        let mut api = Mock::healthy(32, 8);
        assert_eq!(
            check_capacity(&mut api, -1),
            Err(CapacityError::Unavailable)
        );
        assert!(api.calls.is_empty());
    }

    #[test]
    fn resource_values_keep_all_64_bits() {
        let current = u32::MAX as i64 + 5;
        let mut api = Mock::healthy(current + 2, current);
        assert_eq!(check_capacity(&mut api, 2), Ok(()));
        assert_eq!(api.calls, all_calls());
        let mut api = Mock::healthy(current + 1, current);
        assert_eq!(check_capacity(&mut api, 2), Err(CapacityError::Unavailable));
        assert_eq!(api.calls, all_calls());
    }
}
