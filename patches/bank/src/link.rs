#![cfg_attr(target_arch = "arm", no_std)]
#![cfg_attr(target_arch = "arm", no_main)]

// Link roots for real native adapters. This binary is an intermediate ELF,
// never a standalone application or a complete installable Bank patch.
#[cfg(target_arch = "arm")]
mod linked {
    #[used]
    #[link_section = ".bank.link_roots"]
    static NEXT: unsafe extern "aapcs" fn(*mut u8, u32) -> u32 =
        bank_payload::runtime::bank_offline_next;
    #[used]
    #[link_section = ".bank.link_roots"]
    static LOAD: unsafe extern "aapcs" fn(*mut u8) -> u32 =
        bank_payload::runtime::bank_offline_load;
    #[used]
    #[link_section = ".bank.link_roots"]
    static SAVE: unsafe extern "aapcs" fn(*mut u8) -> u32 =
        bank_payload::runtime::bank_offline_save;
    #[used]
    #[link_section = ".bank.link_roots"]
    static STARTUP: unsafe extern "aapcs" fn(*mut u8) =
        patch_common::bootstrap::patch_bootstrap_startup;
    #[used]
    #[link_section = ".bank.link_roots"]
    static MESSAGE: unsafe extern "aapcs" fn() = bank_common::task::bank_message_shown;
    #[used]
    #[link_section = ".bank.link_roots"]
    static NO_GAME: unsafe extern "aapcs" fn() = bank_payload::runtime::bank_offline_no_game;
    #[used]
    #[link_section = ".bank.link_roots"]
    static SCAN: unsafe extern "aapcs" fn(*mut u8) -> u32 =
        bank_payload::runtime::bank_offline_scan;
    #[used]
    #[link_section = ".bank.link_roots"]
    static CLEANUP: unsafe extern "aapcs" fn(*mut u8) -> u32 =
        bank_payload::runtime::bank_offline_cleanup;
    #[used]
    #[link_section = ".bank.link_roots"]
    static VERSION: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32) =
        bank_payload::runtime::bank_version_shown;

    #[used]
    #[link_section = ".bank.link_roots"]
    static REWARDS: unsafe extern "aapcs" fn(*mut u8) -> u32 =
        bank_payload::runtime::bank_offline_rewards;
    #[used]
    #[link_section = ".bank.link_roots"]
    static TIMESTAMP: unsafe extern "aapcs" fn(*mut u8, *mut u8) -> u32 =
        bank_payload::runtime::bank_offline_timestamp;
    #[cfg(feature = "test-build")]
    #[used]
    #[link_section = ".bank.link_roots"]
    static MENU: unsafe extern "aapcs" fn(*mut u8, u32) =
        bank_payload::runtime::bank_test_menu_chosen;
    #[cfg(feature = "test-build")]
    #[used]
    #[link_section = ".bank.link_roots"]
    static MENU_UPDATE: unsafe extern "aapcs" fn(*mut u8) -> u32 =
        bank_payload::runtime::bank_test_menu_update;
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
