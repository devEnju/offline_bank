//! Start-up hook for the reviewed Transporter image.
//!
//! Same method as the Bank patch (patches/bank/src/bootstrap.rs), which has
//! run on a console: `transporter_bootstrap_startup` replaces the one BL to
//! application init at 00103D9C, calls that original function first, then
//! makes the payload's code pages executable. Failure exits the process
//! before control can reach code that cannot run.
//!
//! The code lies in the zero bytes after the original text, which are already
//! executable. The cache system calls require Luma3DS. No heap or thread-local
//! storage is touched.
//!
//! The caches are flushed whole (Luma SVC 0x92 and 0x94), never by address
//! range (0x91, 0x93). The kernel carries a range operation out on every
//! core, by virtual address, whatever process runs there; for this payload's
//! 8 KiB that faulted on core 1 under another process's address space
//! (data abort at 00364000). Bank's range is large enough that the kernel
//! flushes everything anyway. The whole-cache calls involve no address.

#[cfg(target_arch = "arm")]
core::arch::global_asm!(
    r#"
    .syntax unified
    .arch armv6k
    .arm
    .pushsection .transporter.bootstrap, "ax", %progbits
    .balign 4
    .global transporter_bootstrap_startup
    .type transporter_bootstrap_startup, %function
transporter_bootstrap_startup:
    push {r4, lr}
    bl __transporter_original_app_init
    push {r0-r3, r12, lr}
    mrs r4, cpsr
    bl transporter_bootstrap_enable_rx
    cmp r0, #0
    blt .Ltransporter_bootstrap_exit
    msr cpsr_f, r4
    pop {r0-r3, r12, lr}
    pop {r4, pc}
.Ltransporter_bootstrap_exit:
    svc #0x03
    b .Ltransporter_bootstrap_exit
    .size transporter_bootstrap_startup, .-transporter_bootstrap_startup

    .balign 4
    .global transporter_bootstrap_enable_rx
    .type transporter_bootstrap_enable_rx, %function
transporter_bootstrap_enable_rx:
    push {r1-r12, lr}
    mrs r12, cpsr
    push {r12}
    ldr r1, =0xffff8001
    svc #0x27
    cmp r0, #0
    blt .Ltransporter_bootstrap_return
    mov r6, r1
    mov r0, r6
    ldr r1, =__transporter_payload_start
    mov r2, #0
    ldr r3, =__transporter_payload_rx_size
    mov r4, #6
    mov r5, #5
    svc #0x70
    mov r7, r0
    mov r0, r6
    svc #0x23
    cmp r7, #0
    movlt r0, r7
    blt .Ltransporter_bootstrap_return
    cmp r0, #0
    blt .Ltransporter_bootstrap_return
    svc #0x92
    svc #0x94
    mov r0, #0
.Ltransporter_bootstrap_return:
    pop {r12}
    msr cpsr_f, r12
    pop {r1-r12, pc}
    .size transporter_bootstrap_enable_rx, .-transporter_bootstrap_enable_rx
    .ltorg
    .popsection
"#,
    options(raw)
);

#[cfg(target_arch = "arm")]
extern "aapcs" {
    /// # Safety
    /// Only the reviewed app-init call site may call this, with the original
    /// application pointer in `r0`.
    pub fn transporter_bootstrap_startup(app: *mut u8);
}
