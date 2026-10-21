#![cfg_attr(target_arch = "arm", no_std)]
#![cfg_attr(target_arch = "arm", no_main)]

// The link roots of the download patch (`patch_common::link`): the entry
// functions of its profile in the builder and the start-up hook. This binary
// is an intermediate ELF, never a standalone application or a complete
// installable Bank patch.
#[cfg(target_arch = "arm")]
mod linked {
    use bank_download_payload::hooks::*;
    patch_common::link_roots!(
        patch_common::bootstrap::patch_bootstrap_startup,
        bank_download_next,
        bank_version_shown,
        bank_download_open,
        bank_download_open_busy,
        bank_download_scan,
        bank_download_scan_busy,
        bank_download_save,
    );

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
