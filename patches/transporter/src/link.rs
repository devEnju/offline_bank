#![cfg_attr(target_arch = "arm", no_std)]
#![cfg_attr(target_arch = "arm", no_main)]

// Link root for the Transporter hooks. This binary is an intermediate ELF
// that the builder places after the original image, with a paired exheader.
#[cfg(target_arch = "arm")]
mod linked {
    use transporter_payload::cart::{
        transporter_cart_id, transporter_cart_read, transporter_cart_write, transporter_list_next,
        transporter_select,
    };
    use transporter_payload::gen5::transporter_slot_holds;
    use transporter_payload::hooks::{transporter_check, transporter_deliver};
    use transporter_payload::language::{
        transporter_language_buttons, transporter_language_chosen, transporter_language_order,
        transporter_layout_built, transporter_title_language, transporter_vc_listed,
    };

    // Keeps the start-up hook in the link; the builder installs its call.
    #[used]
    #[link_section = ".transporter.link_roots"]
    static STARTUP: unsafe extern "aapcs" fn(*mut u8) =
        transporter_payload::bootstrap::transporter_bootstrap_startup;

    // The fixed entry words the patched sites branch to; the builder checks
    // that they are branches into this image.
    core::arch::global_asm!(
        ".section .transporter.entry,\"ax\",%progbits",
        ".arm",
        ".global transporter_entry",
        "transporter_entry:",
        "b {check}",
        "b {deliver}",
        "b transporter_session",
        "b transporter_slot",
        "b transporter_cart_id_stub",
        "b {cart_read}",
        "b {cart_write}",
        "b transporter_list_next_stub",
        "b transporter_select_stub",
        "b transporter_list_limit_stub",
        "b {language_chosen}",
        "b transporter_language_order_stub",
        "b {language_buttons}",
        "b transporter_list_keep_stub",
        "b transporter_language_back_stub",
        "b transporter_title_begin_stub",
        "b transporter_title_end_stub",
        check = sym transporter_check,
        deliver = sym transporter_deliver,
        cart_read = sym transporter_cart_read,
        cart_write = sym transporter_cart_write,
        language_chosen = sym transporter_language_chosen,
        language_buttons = sym transporter_language_buttons,
    );

    // Call-site adapters for the SD-save hooks: they pass the caller's task
    // pointer (r4) and keep what the replaced instruction did.
    core::arch::global_asm!(
        ".section .text.transporter_cart_stubs,\"ax\",%progbits",
        ".arm",
        // 00243528 (was: bl read-cartridge-id). r0 = out, r4 = cartridge task.
        "transporter_cart_id_stub:",
        "mov r1, r4",
        "b {cart_id}",
        // 00244F10 (was: str r6, [r4, #0x10], state = 3). r4 = list task.
        "transporter_list_next_stub:",
        "mov r0, r4",
        "b {list_next}",
        // 00244908 (was: mov r0, #0). r4 = list task; r5, r6 stay live.
        "transporter_select_stub:",
        "push {{r4, lr}}",
        "mov r0, r4",
        "bl {select}",
        "mov r0, #0",
        "pop {{r4, pc}}",
        // 0024508C (was: ldrh r1, [r1, #0x58], the number of Virtual Console
        // titles found, read as the bound of the loop that appends them).
        // The list holds 40 entries, which the original could not exceed
        // with one cartridge and 39 titles. Once it is full the bound reads
        // as zero and the loop ends. r4 = list task; r0 and r2 stay live.
        "transporter_list_limit_stub:",
        "ldrh r1, [r1, #0x58]",
        "ldrh r12, [r4, #0x64]",
        "cmp r12, #40",
        "movhs r1, #0",
        "bx lr",
        // 00245080 (was: add r1, r1, #1, the list count after a Virtual
        // Console title was appended). The kind byte was just written at
        // kinds[r2] (r5 = kinds); the count only advances when the title is
        // in the listed language, so the next one overwrites it otherwise.
        // r0, the loop index, stays live.
        "transporter_list_keep_stub:",
        "push {{r0-r3, r12, lr}}",
        "ldrb r0, [r5, r2]",
        "bl {vc_listed}",
        "cmp r0, #0",
        "pop {{r0-r3, r12, lr}}",
        "addne r1, r1, #1",
        "bx lr",
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
        "transporter_language_order_stub:",
        "mov r1, r6",
        "b {language_order}",
        // 0022B3B8 (was: b 0022B510, B pressed on a part of the screen that
        // has no Back button; r0 = the part). On the list, part 0, B now
        // presses the Back button, button 7, as the original does for the
        // two other parts at 0022B3BC..0022B3DC. r4 = screen.
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
        "transporter_title_begin_stub:",
        "push {{r0-r3, r12, lr}}",
        "mov r0, #1",
        "bl {title_language}",
        "pop {{r0-r3, r12, lr}}",
        "mov r7, #3",
        "bx lr",
        // 0024BA74 (was: mov r3, #0x27), right after the call.
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
        // get_next_state, leaving the game list (was: connect). Sets bit 1 of
        // the original activity mask through 0022AEEC, as the original connect
        // step did; the original disconnect step clears it. Then answers
        // GET_POKEMON and returns as the original case does.
        "transporter_session:",
        "mov r0, #1",
        "ldr r12, =0x0022aeec",
        "blx r12",
        "mov r0, #9",
        "pop {{r4, pc}}",
        // Gen 5 reader, per-slot result code (was: ldr r0, [r1, r7]). Without
        // a code, a slot that holds no species gets the original skip code
        // 0x14, as the server used to answer. r8 is the slot; in the caller's
        // frame [sp, #0xcc] is the record size and [sp, #0xd8] Box 1. Every
        // register but r0 is returned as it came; the six pushed words keep
        // the stack 8-byte aligned and restore r0 = 0.
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
        // svc 0x23 (CloseHandle) behind an ordinary call boundary.
        ".global transporter_close_handle",
        "transporter_close_handle:",
        "svc #0x23",
        "bx lr",
        ".ltorg",
        slot_holds = sym transporter_slot_holds,
    );

    // Byte-wise memory routines, kept from the builds that had to fit 3.6 KB:
    // they are the ones that have run on a console.
    core::arch::global_asm!(
        ".section .text.transporter_mem,\"ax\",%progbits",
        ".arm",
        ".global memcpy, __aeabi_memcpy, __aeabi_memcpy4, __aeabi_memcpy8",
        ".global memset, __aeabi_memset, __aeabi_memset4, __aeabi_memset8",
        ".global __aeabi_memclr, __aeabi_memclr4, __aeabi_memclr8, memcmp",
        "memcpy:",
        "__aeabi_memcpy:",
        "__aeabi_memcpy4:",
        "__aeabi_memcpy8:",
        "mov r3, r0",
        "1: subs r2, r2, #1",
        "bxlo lr",
        "ldrb r12, [r1], #1",
        "strb r12, [r3], #1",
        "b 1b",
        "memset:",
        "mov r3, r0",
        "2: subs r2, r2, #1",
        "bxlo lr",
        "strb r1, [r3], #1",
        "b 2b",
        "__aeabi_memclr:",
        "__aeabi_memclr4:",
        "__aeabi_memclr8:",
        "mov r2, #0",
        "__aeabi_memset:",
        "__aeabi_memset4:",
        "__aeabi_memset8:",
        "3: subs r1, r1, #1",
        "bxlo lr",
        "strb r2, [r0], #1",
        "b 3b",
        "memcmp:",
        "mov r3, r0",
        "4: subs r2, r2, #1",
        "movlo r0, #0",
        "bxlo lr",
        "ldrb r0, [r3], #1",
        "ldrb r12, [r1], #1",
        "subs r0, r0, r12",
        "beq 4b",
        "bx lr",
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
