//! The start-up hook, the same for every patch of Bank and of Transporter.
//!
//! `patch_bootstrap_startup` replaces the one BL to application init in the
//! original's main function. It calls that original function first, then
//! makes the payload's code pages executable. Failure exits the process
//! before control can reach code that cannot run.
//!
//! The code lies in the zero bytes after the original text, which are
//! already executable, in the section `.patch.bootstrap`. A patch that needs
//! more code there (Transporter's layout check) puts it into
//! `.patch.bootstrap.*`, which its linker script places after the hook. The
//! linker script names the original's application init
//! (`__patch_original_app_init`) and the payload's bounds
//! (`__patch_payload_start`, `__patch_payload_rx_size`).
//!
//! `patch_bootstrap_enable_rx` returns a native result in r0 and preserves
//! r1-r12, SP, LR and the NZCVQ flags. Both need an 8-byte-aligned valid
//! stack. No heap or thread-local storage is touched.
//!
//! The caches are flushed whole (Luma SVC 0x92 and 0x94), never by address
//! range (0x91, 0x93). The kernel carries a small range out by virtual
//! address on every core, whatever process runs there; for Transporter's
//! few pages that faulted on core 1 under another process's address space
//! (data abort at 00364000). The whole-cache calls involve no address. They
//! require Luma3DS.

#[cfg(target_arch = "arm")]
core::arch::global_asm!(
    r#"
    .syntax unified
    .arch armv6k
    .arm
    .pushsection .patch.bootstrap, "ax", %progbits
    .balign 4
    .global patch_bootstrap_startup
    .type patch_bootstrap_startup, %function
patch_bootstrap_startup:
    push {r4, lr}
    bl __patch_original_app_init
    push {r0-r3, r12, lr}
    mrs r4, cpsr
    bl patch_bootstrap_enable_rx
    cmp r0, #0
    blt .Lpatch_bootstrap_exit
    msr cpsr_f, r4
    pop {r0-r3, r12, lr}
    pop {r4, pc}
.Lpatch_bootstrap_exit:
    svc #0x03
    b .Lpatch_bootstrap_exit
    .size patch_bootstrap_startup, .-patch_bootstrap_startup

    .balign 4
    .global patch_bootstrap_enable_rx
    .type patch_bootstrap_enable_rx, %function
patch_bootstrap_enable_rx:
    push {r1-r12, lr}
    mrs r12, cpsr
    push {r12}
    ldr r1, =0xffff8001
    svc #0x27
    cmp r0, #0
    blt .Lpatch_bootstrap_return
    mov r6, r1
    mov r0, r6
    ldr r1, =__patch_payload_start
    mov r2, #0
    ldr r3, =__patch_payload_rx_size
    mov r4, #6
    mov r5, #5
    svc #0x70
    mov r7, r0
    mov r0, r6
    svc #0x23
    cmp r7, #0
    movlt r0, r7
    blt .Lpatch_bootstrap_return
    cmp r0, #0
    blt .Lpatch_bootstrap_return
    svc #0x92
    svc #0x94
    mov r0, #0
.Lpatch_bootstrap_return:
    pop {r12}
    msr cpsr_f, r12
    pop {r1-r12, pc}
    .size patch_bootstrap_enable_rx, .-patch_bootstrap_enable_rx
    .ltorg
    .popsection
"#,
    options(raw)
);

#[cfg(target_arch = "arm")]
extern "aapcs" {
    /// # Safety
    /// Only the original's one call of application init may call this, with
    /// the application pointer in `r0`; the builder places that call.
    pub fn patch_bootstrap_startup(app: *mut u8);
}
