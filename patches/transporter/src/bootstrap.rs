//! Transporter's code beside the start-up hook (`patch_common::bootstrap`),
//! in the zero bytes after the original text, which are executable from the
//! start.
//!
//! `transporter_layout_check_stub` stands at 0013AEE4, on the way of every
//! layout the application builds, and the first layouts are built during start-up,
//! before the payload can run: branching into the payload from there was a
//! prefetch abort at its entry word. The check runs in this already
//! executable place and enters the payload only for the language screen's
//! layout, which is built when that screen is opened.

#[cfg(target_arch = "arm")]
core::arch::global_asm!(
    r#"
    .syntax unified
    .arch armv6k
    .arm
    .pushsection .patch.bootstrap.check, "ax", %progbits

    @ 0013AEE4 (was: mov r1, r0, the layout binary on its way to
    @ Layout::Build); the builder checks that this is its first
    @ instruction. A layout binary starts with "CLYT" and has
    @ its size at +0xC; only the language screen's (0x3188 bytes) goes on to
    @ the payload, which checks its content. r2 and r12 are free here: the
    @ original sets r2 next and does not use r12.
    .balign 4
    .global transporter_layout_check_stub
    .type transporter_layout_check_stub, %function
transporter_layout_check_stub:
    mov r1, r0
    cmp r0, #0
    bxeq lr
    ldr r12, [r0, #0xc]
    ldr r2, =0x3188
    cmp r12, r2
    bxne lr
    ldr r12, =transporter_layout_adjust_stub
    bx r12
    .ltorg
    .size transporter_layout_check_stub, .-transporter_layout_check_stub
    .popsection
"#,
    options(raw)
);

#[cfg(target_arch = "arm")]
unsafe extern "aapcs" {
    /// # Safety
    /// Only the patched site 0013AEE4 may enter it, with its registers.
    pub fn transporter_layout_check_stub();
}
