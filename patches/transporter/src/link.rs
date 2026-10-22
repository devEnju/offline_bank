#![cfg_attr(target_arch = "arm", no_std)]
#![cfg_attr(target_arch = "arm", no_main)]

// The link roots of the Transporter patch (`patch_common::link`): the entry
// functions of its profile in the builder and the start-up hook. This binary
// is an intermediate ELF that the builder places after the original image,
// with a paired exheader.
#[cfg(target_arch = "arm")]
mod linked {
    use transporter_payload::{
        bootstrap::*, cart::*, gen5::*, hooks::*, language::*, legacy_shiny::*,
    };
    patch_common::link_roots!(
        patch_common::bootstrap::patch_bootstrap_startup,
        transporter_next,
        transporter_list_ready_stub,
        transporter_list_indicator_stub,
        transporter_list_frame_stub,
        transporter_message_stub,
        transporter_message_text,
        transporter_search,
        transporter_read,
        transporter_question,
        transporter_transfer,
        transporter_slot_stub,
        transporter_cart_id_stub,
        transporter_cart_read,
        transporter_cart_write,
        transporter_list_next_stub,
        transporter_select_stub,
        transporter_card_removed,
        transporter_entry_card_removed,
        transporter_language_chosen,
        transporter_language_order_stub,
        transporter_language_buttons,
        transporter_language_back_stub,
        transporter_start_screen_begin_stub,
        transporter_start_screen_end_stub,
        transporter_version_shown,
        transporter_layout_check_stub,
        transporter_vc_mount_stub,
        transporter_vc_scan_stub,
        transporter_deliver_stub,
        transporter_shiny_build_stub,
        transporter_shiny_gender_stub,
    );

    #[panic_handler]
    fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
        // A panic cannot fall through into removing Pokémon from the game.
        loop {
            unsafe { core::arch::asm!("svc #0x03", options(nomem, nostack)) };
        }
    }
}

#[cfg(not(target_arch = "arm"))]
fn main() {
    panic!("transporter-payload-link requires the reviewed ARMv6K target and linker script");
}
