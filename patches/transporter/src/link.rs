#![cfg_attr(target_arch = "arm", no_std)]
#![cfg_attr(target_arch = "arm", no_main)]

// Link root for the Transporter hooks. This binary is an intermediate ELF
// that the builder places after the original image, with a paired exheader.
#[cfg(target_arch = "arm")]
mod linked {
    use transporter_payload::cart::{
        transporter_card_removed, transporter_cart_id, transporter_cart_read,
        transporter_cart_write, transporter_entry_card_removed, transporter_list_next,
        transporter_select,
    };
    use transporter_payload::gen5::transporter_slot_holds;
    use transporter_payload::hooks::{
        transporter_check, transporter_deliver, transporter_list_frame, transporter_list_indicator,
        transporter_list_ready, transporter_list_started, transporter_message_waits,
        transporter_next, transporter_version,
    };
    use transporter_payload::language::{
        transporter_language_buttons, transporter_language_chosen, transporter_language_order,
        transporter_layout_built, transporter_title_language, transporter_vc_listed,
        transporter_vc_wanted,
    };
    use transporter_payload::legacy_shiny::{
        transporter_shiny_build, transporter_shiny_keeps_gender,
    };

    // Keeps the start-up hook and the entries that are Rust functions in
    // the link; the patched sites call them by name (the builder's profile,
    // `transporter15::ENTRIES`). The stubs below are kept by the linker
    // script.
    #[used]
    #[link_section = ".transporter.link_roots"]
    static STARTUP: unsafe extern "aapcs" fn(*mut u8) =
        patch_common::bootstrap::patch_bootstrap_startup;
    #[used]
    #[link_section = ".transporter.link_roots"]
    static NEXT: unsafe extern "aapcs" fn(*mut u8, u32) -> u32 = transporter_next;
    #[used]
    #[link_section = ".transporter.link_roots"]
    static CART_READ: unsafe extern "aapcs" fn(u32, u32, *mut u8, u32) -> i32 =
        transporter_cart_read;
    #[used]
    #[link_section = ".transporter.link_roots"]
    static CART_WRITE: unsafe extern "aapcs" fn(u32, u32, *const u8, u32) -> i32 =
        transporter_cart_write;
    #[used]
    #[link_section = ".transporter.link_roots"]
    static CARD_REMOVED: unsafe extern "aapcs" fn(*mut u8) -> u32 = transporter_card_removed;
    #[used]
    #[link_section = ".transporter.link_roots"]
    static ENTRY_CARD_REMOVED: unsafe extern "aapcs" fn(*const u8) -> u32 =
        transporter_entry_card_removed;
    #[used]
    #[link_section = ".transporter.link_roots"]
    static LANGUAGE_CHOSEN: extern "aapcs" fn(u32) = transporter_language_chosen;
    #[used]
    #[link_section = ".transporter.link_roots"]
    static LANGUAGE_BUTTONS: unsafe extern "aapcs" fn(*mut u8) = transporter_language_buttons;
    #[used]
    #[link_section = ".transporter.link_roots"]
    static VERSION: unsafe extern "aapcs" fn(*mut u8, u32, u32, u32) = transporter_version;

    // Call-site adapters for the SD-save hooks: they pass the caller's task
    // pointer (r4) and keep what the replaced instruction did.
    core::arch::global_asm!(
        ".section .text.transporter_cart_stubs,\"ax\",%progbits",
        ".arm",
        // 00243528 (was: bl read-cartridge-id). r0 = out, r4 = cartridge task.
        ".global transporter_cart_id_stub",
        ".type transporter_cart_id_stub, %function",
        "transporter_cart_id_stub:",
        "mov r1, r4",
        "b {cart_id}",
        // 00244F10 (was: str r6, [r4, #0x10], state = 3). r4 = list task.
        ".size transporter_cart_id_stub, .-transporter_cart_id_stub",
        ".global transporter_list_next_stub",
        ".type transporter_list_next_stub, %function",
        "transporter_list_next_stub:",
        "mov r0, r4",
        "b {list_next}",
        // 00244908 (was: mov r0, #0). r4 = list task; r5, r6 stay live.
        ".size transporter_list_next_stub, .-transporter_list_next_stub",
        ".global transporter_select_stub",
        ".type transporter_select_stub, %function",
        "transporter_select_stub:",
        "push {{r4, lr}}",
        "mov r0, r4",
        "bl {select}",
        "mov r0, #0",
        "pop {{r4, pc}}",
        // 002412D4 (was: bl 002512A8, "is the title of this row installed?",
        // r0 = version, r1 = language id). A row in another language than
        // the listed one answers no; the original answers for the others.
        // It runs on the scanner's thread and only reads the filter.
        ".size transporter_select_stub, .-transporter_select_stub",
        ".global transporter_vc_scan_stub",
        ".type transporter_vc_scan_stub, %function",
        "transporter_vc_scan_stub:",
        "push {{r0, r1, r4, lr}}",
        "mov r0, r1",
        "bl {vc_listed}",
        "movs r12, r0",
        "pop {{r0, r1, r4, lr}}",
        "moveq r0, #0",
        "bxeq lr",
        "ldr r12, =0x002512a8",
        "bx r12",
        ".size transporter_vc_scan_stub, .-transporter_vc_scan_stub",
        ".ltorg",
        cart_id = sym transporter_cart_id,
        list_next = sym transporter_list_next,
        select = sym transporter_select,
        vc_listed = sym transporter_vc_listed,
    );

    // The language screen (0022BD28).
    core::arch::global_asm!(
        ".section .text.transporter_language_stubs,\"ax\",%progbits",
        ".arm",
        // 0022BEE0 (was: bl 0022BC0C). r0 = screen; r6 = the constructor's
        // parameter, whose heap the screen's objects come from.
        ".global transporter_language_order_stub",
        ".type transporter_language_order_stub, %function",
        "transporter_language_order_stub:",
        "mov r1, r6",
        "b {language_order}",
        // 0022B3B8 (was: b 0022B510, B pressed on a part of the screen that
        // has no Back button; r0 = the part). On the list, part 0, B now
        // presses the Back button, button 7, as the original does for the
        // two other parts at 0022B3BC..0022B3DC. r4 = screen.
        ".size transporter_language_order_stub, .-transporter_language_order_stub",
        ".global transporter_language_back_stub",
        ".type transporter_language_back_stub, %function",
        "transporter_language_back_stub:",
        "cmp r0, #0",
        "ldrne r12, =0x0022b510",
        "bxne r12",
        "ldr r0, [r4, #0x10]",
        "mov r1, #7",
        "ldr r12, =0x0022b3dc",
        "bx r12",
        // The title screen's constructor (0024B9E4), around its call that
        // loads its archives and builds its layouts (0024BA70).
        // 0024BA44 (was: mov r7, #3), while the call's arguments are set up;
        // every register is returned as it came.
        ".size transporter_language_back_stub, .-transporter_language_back_stub",
        ".global transporter_title_begin_stub",
        ".type transporter_title_begin_stub, %function",
        "transporter_title_begin_stub:",
        "push {{r0-r3, r12, lr}}",
        "mov r0, #1",
        "bl {title_language}",
        "pop {{r0-r3, r12, lr}}",
        "mov r7, #3",
        "bx lr",
        // 0024BA74 (was: mov r3, #0x27), right after the call.
        ".size transporter_title_begin_stub, .-transporter_title_begin_stub",
        ".global transporter_title_end_stub",
        ".type transporter_title_end_stub, %function",
        "transporter_title_end_stub:",
        "push {{r0-r3, r12, lr}}",
        "mov r0, #0",
        "bl {title_language}",
        "pop {{r0-r3, r12, lr}}",
        "mov r3, #0x27",
        "bx lr",
        // Reached from transporter_layout_check (bootstrap.rs) for a layout
        // binary of the language screen's size, with the lr of the patched
        // site 0013AEE4. r0 and r1 are the layout binary and are returned as
        // they came, with every other register.
        ".global transporter_layout_adjust",
        ".size transporter_title_end_stub, .-transporter_title_end_stub",
        "transporter_layout_adjust:",
        "push {{r0-r3, r12, lr}}",
        "bl {layout_built}",
        "pop {{r0-r3, r12, lr}}",
        "bx lr",
        ".ltorg",
        language_order = sym transporter_language_order,
        layout_built = sym transporter_layout_built,
        title_language = sym transporter_title_language,
    );

    // Small pieces that run inside original functions and use their state.
    core::arch::global_asm!(
        ".section .text.transporter_stubs,\"ax\",%progbits",
        ".arm",
        // Game list set-up, state 0: the call that starts its indicator
        // (00244EAC, was: bl 002450F4, r0 = task), once the list's screen is
        // built. While the search's panel is up (entry: 1) the list starts
        // nothing: on inside 002450F4 with that function's own prologue at
        // 00245180, which only notes when the list began and returns to 2:;
        // the started entry then moves that time back to when the panel
        // went up and returns to the caller. Otherwise the call.
        ".global transporter_list_indicator_stub",
        ".type transporter_list_indicator_stub, %function",
        "transporter_list_indicator_stub:",
        "push {{r0, lr}}",
        "bl {list_indicator}",
        "movs r12, r0",
        "pop {{r0, lr}}",
        "ldreq r12, =0x002450f4",
        "bxeq r12",
        "push {{r0, lr}}",
        "adr lr, 2f",
        "push {{r3-r11, lr}}",
        "mov r4, r0",
        "ldr r12, =0x00245180",
        "bx r12",
        "2:",
        "pop {{r0, lr}}",
        "b {list_started}",
        ".size transporter_list_indicator_stub, .-transporter_list_indicator_stub",
        // The scan of the Virtual Console saves (001E419C), for title r5:
        // the call that mounts its save (001E41C0, was: bl 0010E4A4, r0 = the
        // mount name, r1 = the title). Application init's scan, the one
        // before the payload can run, is skipped (0010621C). A title the
        // entry does not want: on
        // with the next title (001E42D0), with nothing mounted or unmounted;
        // its entry in the scan's table was cleared at 001E41B4. Otherwise
        // the call, which returns to the scan.
        ".global transporter_vc_mount_stub",
        ".type transporter_vc_mount_stub, %function",
        "transporter_vc_mount_stub:",
        "push {{r0, r1, r12, lr}}",
        "mov r0, r5",
        "bl {vc_wanted}",
        "cmp r0, #0",
        "pop {{r0, r1, r12, lr}}",
        "ldreq r12, =0x001e42d0",
        "bxeq r12",
        "ldr r12, =0x0010e4a4",
        "bx r12",
        ".size transporter_vc_mount_stub, .-transporter_vc_mount_stub",
        // Game list set-up, every frame: its check that restarts its
        // indicator's sound (00244D40, was: ldr r0, [r5, #0x5c]; r4 = task,
        // r5 = the list's screen; the check's own calls clobber r12 and the
        // flags). While the search's panel is up (entry: 1) on behind the
        // check (00244D8C); otherwise the instruction and the check.
        ".global transporter_list_frame_stub",
        ".type transporter_list_frame_stub, %function",
        "transporter_list_frame_stub:",
        "push {{r0-r3, r12, lr}}",
        "bl {list_frame}",
        "cmp r0, #0",
        "pop {{r0-r3, r12, lr}}",
        "ldrne r12, =0x00244d8c",
        "bxne r12",
        "ldr r0, [r5, #0x5c]",
        "ldr r12, =0x00244d44",
        "bx r12",
        ".size transporter_list_frame_stub, .-transporter_list_frame_stub",
        ".ltorg",
        // Game list, the first instruction of its update (002445EC, was:
        // ldr r0, [r4, #0x38]): its set-up is done and it is on screen.
        // r4 = task; the instruction is run here and every other register
        // the original still uses is returned as it came.
        ".global transporter_list_ready_stub",
        ".type transporter_list_ready_stub, %function",
        "transporter_list_ready_stub:",
        "push {{r1-r3, r12, lr}}",
        "sub sp, sp, #4",
        "bl {list_ready}",
        "add sp, sp, #4",
        "pop {{r1-r3, r12, lr}}",
        "ldr r0, [r4, #0x38]",
        "bx lr",
        // The original's "show a message" function, its first instruction
        // (0019B50C, was: push {r4-r8, lr}). r0 = ui, r1 = message, r2 =
        // whether the message waits for an answer; lr is the caller's. The
        // instruction is run here; for a message that waits the entry lets
        // HOME and sleep through; then on inside the original with every
        // register as it came.
        ".size transporter_list_ready_stub, .-transporter_list_ready_stub",
        ".global transporter_message_stub",
        ".type transporter_message_stub, %function",
        "transporter_message_stub:",
        "push {{r4, r5, r6, r7, r8, lr}}",
        "cmp r2, #0",
        "beq 5f",
        "push {{r0-r3, r12, lr}}",
        "bl {message_waits}",
        "pop {{r0-r3, r12, lr}}",
        ".size transporter_message_stub, .-transporter_message_stub",
        "5:",
        "ldr r12, =0x0019b510",
        "bx r12",
        // Game search, first step (00246F48, was: ldr r0, [r0], the start
        // of the cartridge scan). r4 = task; lr is free, the original
        // function saved its own. 1: still checking, the search's "nothing
        // this frame" return. 2: a message is on screen; 00247000 selects
        // the search's own step that waits for it and ends the search.
        // 3: its ending without a message. Else: the replaced instruction,
        // with r0 as the search had loaded it, and on with the search.
        ".global transporter_check_stub",
        ".type transporter_check_stub, %function",
        "transporter_check_stub:",
        "mov r0, r4",
        "bl {check}",
        "cmp r0, #1",
        "ldreq r12, =0x00247024",
        "bxeq r12",
        "cmp r0, #2",
        "ldreq r12, =0x00247000",
        "bxeq r12",
        "cmp r0, #3",
        "ldreq r12, =0x0024702c",
        "bxeq r12",
        "ldr r0, =0x00329380",
        "ldr r0, [r0]",
        "ldr r12, =0x00246f4c",
        "bx r12",
        // Transfer, sub-state 0 (0024A150, was: the start of creating the
        // upload request). r4 = task; lr is free as above. 1: delivered, on
        // to the original removal code. 2: still working, the original
        // return, to come back next frame. Else: sub-state 0x11, the
        // original failure message, through the store at 0024A4C0.
        ".size transporter_check_stub, .-transporter_check_stub",
        ".global transporter_deliver_stub",
        ".type transporter_deliver_stub, %function",
        "transporter_deliver_stub:",
        "mov r0, r4",
        "bl {deliver}",
        "cmp r0, #1",
        "ldreq r12, =0x0024a274",
        "bxeq r12",
        "cmp r0, #2",
        "ldreq r12, =0x0024a5dc",
        "bxeq r12",
        "mov r0, #0x11",
        "ldr r12, =0x0024a4c0",
        "bx r12",
        // Gen 5 reader, per-slot result code (was: ldr r0, [r1, r7]). Without
        // a code, a slot that holds no species gets the original skip code
        // 0x14, as the server used to answer. r8 is the slot; in the caller's
        // frame [sp, #0xcc] is the record size and [sp, #0xd8] Box 1. Every
        // register but r0 is returned as it came; the six pushed words keep
        // the stack 8-byte aligned and restore r0 = 0.
        ".size transporter_deliver_stub, .-transporter_deliver_stub",
        ".global transporter_slot",
        ".type transporter_slot, %function",
        "transporter_slot:",
        "ldr r0, [r1, r7]",
        "cmp r0, #0",
        "bxne lr",
        "push {{r0-r3, r12, lr}}",
        "ldr r2, [sp, #0xcc + 24]",
        "ldr r3, [sp, #0xd8 + 24]",
        "mla r0, r2, r8, r3",
        "bl {slot_holds}",
        "cmp r0, #0",
        "pop {{r0-r3, r12, lr}}",
        "moveq r0, #0x14",
        "bx lr",
        ".size transporter_slot, .-transporter_slot",
        ".ltorg",
        check = sym transporter_check,
        message_waits = sym transporter_message_waits,
        list_ready = sym transporter_list_ready,
        list_indicator = sym transporter_list_indicator,
        list_frame = sym transporter_list_frame,
        list_started = sym transporter_list_started,
        vc_wanted = sym transporter_vc_wanted,
        deliver = sym transporter_deliver,
        slot_holds = sym transporter_slot_holds,
    );

    // The Gen 1 converter (0024F428), the Easter egg in legacy_shiny.rs.
    // r4 is the source record at both sites.
    core::arch::global_asm!(
        ".section .text.transporter_shiny_stubs,\"ax\",%progbits",
        ".arm",
        // 0024F6B0 (was: bl 0019CD3C, the Pokémon constructor; r2 = the
        // construction parameters). The entry may ask for a shiny; then the
        // constructor with every register as it came, lr included, so that
        // it returns to 0024F6B4.
        ".global transporter_shiny_build_stub",
        ".type transporter_shiny_build_stub, %function",
        "transporter_shiny_build_stub:",
        "push {{r0-r3, r12, lr}}",
        "mov r1, r2",
        "mov r0, r4",
        "bl {shiny_build}",
        "pop {{r0-r3, r12, lr}}",
        "ldr r12, =0x0019cd3c",
        "bx r12",
        // 0024F8E0 (was: ldrb r1, [r4, #0x1b], the start of the gender
        // from the Attack DV; r0 = its threshold). lr and the flags are not
        // live here. Kept gender: on at 0024F910, behind the gender code.
        // Else the instruction, and back to 0024F8E4.
        ".size transporter_shiny_build_stub, .-transporter_shiny_build_stub",
        ".global transporter_shiny_gender_stub",
        ".type transporter_shiny_gender_stub, %function",
        "transporter_shiny_gender_stub:",
        "push {{r0-r3, r12, lr}}",
        "mov r0, r4",
        "bl {shiny_keeps_gender}",
        "movs r12, r0",
        "pop {{r0-r3, r12, lr}}",
        "ldrne r12, =0x0024f910",
        "bxne r12",
        "ldrb r1, [r4, #0x1b]",
        "bx lr",
        ".size transporter_shiny_gender_stub, .-transporter_shiny_gender_stub",
        ".ltorg",
        shiny_build = sym transporter_shiny_build,
        shiny_keeps_gender = sym transporter_shiny_keeps_gender,
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
