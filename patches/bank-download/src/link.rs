#![cfg_attr(target_arch = "arm", no_std)]
#![cfg_attr(target_arch = "arm", no_main)]

// Link roots of the download patch. This binary is an intermediate ELF,
// never a standalone application or a complete installable Bank patch.
#[cfg(target_arch = "arm")]
mod linked {
    #[used]
    #[link_section = ".bank.link_roots"]
    static NEXT: unsafe extern "aapcs" fn(*mut u8, u32) -> u32 =
        bank_download_payload::runtime::bank_download_next;
    #[used]
    #[link_section = ".bank.link_roots"]
    static SCAN: unsafe extern "aapcs" fn(*mut u8) -> u32 =
        bank_download_payload::runtime::bank_download_scan;
    #[used]
    #[link_section = ".bank.link_roots"]
    static SCAN_BUSY: unsafe extern "aapcs" fn(*mut u8) -> u32 =
        bank_download_payload::runtime::bank_download_scan_busy;
    #[used]
    #[link_section = ".bank.link_roots"]
    static OPEN: unsafe extern "aapcs" fn(*mut u8) -> u32 =
        bank_download_payload::runtime::bank_download_open;
    #[used]
    #[link_section = ".bank.link_roots"]
    static OPEN_BUSY: unsafe extern "aapcs" fn(*mut u8) -> u32 =
        bank_download_payload::runtime::bank_download_open_busy;
    #[used]
    #[link_section = ".bank.link_roots"]
    static SAVE: unsafe extern "aapcs" fn(*mut u8) -> u32 =
        bank_download_payload::runtime::bank_download_save;
    #[used]
    #[link_section = ".bank.link_roots"]
    static STARTUP: unsafe extern "aapcs" fn(*mut u8) =
        patch_common::bootstrap::patch_bootstrap_startup;
    #[used]
    #[link_section = ".bank.link_roots"]
    static VERSION: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32) =
        bank_download_payload::runtime::bank_version_shown;

    #[panic_handler]
    fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
        // A panic cannot fall through into a session left half way.
        loop {
            unsafe { core::arch::asm!("svc #0x03", options(nomem, nostack)) };
        }
    }
}

#[cfg(not(target_arch = "arm"))]
fn main() {
    panic!("bank-download-payload-link requires the reviewed ARMv6K target and linker script");
}
