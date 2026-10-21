#![cfg_attr(target_arch = "arm", no_std)]
#![cfg_attr(target_arch = "arm", no_main)]

// The link roots of the migration patch (`patch_common::link`): the entry
// functions of its profile in the builder and the start-up hook. This binary
// is an intermediate ELF, never a standalone application or a complete
// installable Bank patch.
#[cfg(target_arch = "arm")]
mod linked {
    use bank_migrate_payload::hooks::*;
    patch_common::link_roots!(
        patch_common::bootstrap::patch_bootstrap_startup,
        bank_migrate_next,
        bank_common::task::bank_message_stub,
        bank_version_shown,
        bank_migrate_scan,
    );

    #[panic_handler]
    fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
        // A panic cannot fall through into a partly converted Bank.
        loop {
            unsafe { core::arch::asm!("svc #0x03", options(nomem, nostack)) };
        }
    }
}

#[cfg(not(target_arch = "arm"))]
fn main() {
    panic!("bank-migrate-payload-link requires the reviewed ARMv6K target and linker script");
}
