//! Bank's code beside the start-up hook (`patch_common::bootstrap`), in the
//! zero bytes after the original text, which are executable from the start.
//!
//! `bank_layout_check_stub` stands in the layout vtable for the parser
//! `0024f1f8` (slot `0035f118`), on the way of every layout the application
//! builds, and the first layouts are built during start-up, before the
//! payload can run: entering the payload from there is a prefetch abort at
//! its entry word. The check runs in this already executable place and
//! enters the payload only for the transfer screen's layout, which is built
//! when that screen is opened.

#[cfg(target_arch = "arm")]
core::arch::global_asm!(
    r#"
    .syntax unified
    .arch armv6k
    .arm
    .pushsection .patch.bootstrap.check, "ax", %progbits

    @ The parser's arguments: r0 the layout, r1 the layout binary, r2 the
    @ resource accessor, r3 unused. A layout binary starts with "CLYT" and
    @ has its size at +0xC; only the transfer screen's (28,392 = 0x6EE8
    @ bytes) goes on to the payload, which checks its content. r12 is free at
    @ a call; the builder and the verifier check the first instruction.
    .balign 4
    .global bank_layout_check_stub
    .type bank_layout_check_stub, %function
bank_layout_check_stub:
    cmp r1, #0
    beq 1f
    ldr r12, [r1, #0xc]
    sub r12, r12, #0x6e00
    cmp r12, #0xe8
    ldreq r12, =bank_layout_build
    bxeq r12
1:
    ldr r12, =0x0024f1f8
    bx r12
    .ltorg
    .size bank_layout_check_stub, .-bank_layout_check_stub
    .popsection
"#,
    options(raw)
);

#[cfg(target_arch = "arm")]
unsafe extern "aapcs" {
    /// # Safety
    /// Only the layout vtable's slot `0035f118` may enter it, with the
    /// parser's arguments.
    pub fn bank_layout_check_stub();
}
