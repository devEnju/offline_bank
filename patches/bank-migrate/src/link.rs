#![cfg_attr(target_arch = "arm", no_std)]
#![cfg_attr(target_arch = "arm", no_main)]

// Link roots of the migration patch. This binary is an intermediate ELF,
// never a standalone application or a complete installable Bank patch.
#[cfg(target_arch = "arm")]
mod linked {
    #[used]
    #[link_section = ".bank.link_roots"]
    static NEXT: unsafe extern "aapcs" fn(*mut u8, u32) -> u32 =
        bank_migrate_payload::runtime::bank_migrate_next;
    #[used]
    #[link_section = ".bank.link_roots"]
    static OPEN: unsafe extern "aapcs" fn(*mut u8) -> u32 =
        bank_migrate_payload::runtime::bank_migrate_open;
    #[used]
    #[link_section = ".bank.link_roots"]
    static STARTUP: unsafe extern "aapcs" fn(*mut u8) =
        bank_common::bootstrap::bank_bootstrap_startup;

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
