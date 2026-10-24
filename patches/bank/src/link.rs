#![cfg_attr(target_arch = "arm", no_std)]
#![cfg_attr(target_arch = "arm", no_main)]

// The link roots of the offline patch (`patch_common::link`): the entry
// functions of its profile in the builder and the start-up hook. This binary
// is an intermediate ELF, never a standalone application or a complete
// installable Bank patch.
#[cfg(target_arch = "arm")]
mod linked {
    use bank_payload::hooks::*;
    patch_common::link_roots!(
        patch_common::bootstrap::patch_bootstrap_startup,
        bank_offline_next,
        bank_message_stub,
        bank_version_shown,
        bank_offline_no_game_stub,
        bank_offline_timestamp,
        bank_offline_scan,
        bank_offline_rewards,
        bank_offline_load,
        bank_offline_save,
    );
    #[cfg(feature = "test-build")]
    mod test_build {
        use bank_payload::hooks::*;
        patch_common::link_roots!(bank_test_menu_update, bank_test_menu_chosen);
    }

    #[panic_handler]
    fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
        // Panic cannot fall through into native saving or a partly updated Bank.
        loop {
            unsafe { core::arch::asm!("svc #0x03", options(nomem, nostack)) };
        }
    }
}

#[cfg(not(target_arch = "arm"))]
fn main() {
    panic!("bank-payload-link requires the reviewed ARMv6K target and linker script");
}
