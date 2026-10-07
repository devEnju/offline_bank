//! Bootstrap for the reviewed ARM Bank image; not an installed hook.
//!
//! `bank_bootstrap_startup` replaces only the reviewed BL to application init.
//! It calls that original function first, then enables payload RX pages. Failure
//! exits the process before control can reach an unexecutable offline payload.
//!
//! `bank_bootstrap_enable_rx` returns a native Result in r0 and preserves
//! r1-r12, SP/LR, and NZCVQ flags. Both require an 8-byte-aligned valid stack.
//! Custom cache SVCs require the audited Luma extension. Linker symbols provide
//! exact page-aligned payload boundaries; no heap or TLS accesses are made.
//!
//! The range cache calls (0x91, 0x93) are safe here only because the range is
//! large: the kernel then flushes the whole cache. A small range is flushed
//! by virtual address on every core and faults under another process's
//! address space (seen with Transporter's 8 KiB). The linker script asserts
//! the size; a smaller payload must use 0x92 and 0x94 as Transporter does.

#[cfg(target_arch = "arm")]
core::arch::global_asm!(
    r#"
    .syntax unified
    .arch armv6k
    .arm
    .pushsection .bank.bootstrap, "ax", %progbits
    .balign 4
    .global bank_bootstrap_startup
    .type bank_bootstrap_startup, %function
bank_bootstrap_startup:
    push {r4, lr}
    bl __bank_original_app_init
    push {r0-r3, r12, lr}
    mrs r4, cpsr
    bl bank_bootstrap_enable_rx
    cmp r0, #0
    blt .Lbank_bootstrap_exit
    msr cpsr_f, r4
    pop {r0-r3, r12, lr}
    pop {r4, pc}
.Lbank_bootstrap_exit:
    svc #0x03
    b .Lbank_bootstrap_exit
    .size bank_bootstrap_startup, .-bank_bootstrap_startup

    .balign 4
    .global bank_bootstrap_enable_rx
    .type bank_bootstrap_enable_rx, %function
bank_bootstrap_enable_rx:
    push {r1-r12, lr}
    mrs r12, cpsr
    push {r12}
    ldr r1, =0xffff8001
    svc #0x27
    cmp r0, #0
    blt .Lbank_bootstrap_return
    mov r6, r1
    mov r0, r6
    ldr r1, =__bank_payload_start
    mov r2, #0
    ldr r3, =__bank_payload_rx_size
    mov r4, #6
    mov r5, #5
    svc #0x70
    mov r7, r0
    mov r0, r6
    svc #0x23
    cmp r7, #0
    movlt r0, r7
    blt .Lbank_bootstrap_return
    cmp r0, #0
    blt .Lbank_bootstrap_return
    ldr r0, =__bank_payload_start
    ldr r1, =__bank_payload_rx_size
    svc #0x91
    ldr r0, =__bank_payload_start
    ldr r1, =__bank_payload_rx_size
    svc #0x93
    mov r0, #0
.Lbank_bootstrap_return:
    pop {r12}
    msr cpsr_f, r12
    pop {r1-r12, pc}
    .size bank_bootstrap_enable_rx, .-bank_bootstrap_enable_rx
    .ltorg
    .popsection
"#,
    options(raw)
);

#[cfg(target_arch = "arm")]
extern "aapcs" {
    /// # Safety
    /// Requires the reviewed layout, supported Luma SVCs, and a valid aligned
    /// native stack. Execute from original RX memory before any payload call.
    pub fn bank_bootstrap_enable_rx() -> i32;

    /// # Safety
    /// Only the reviewed app-init callsite may call this with the native app
    /// pointer in r0. The startup hook itself is installed by a separate profile.
    pub fn bank_bootstrap_startup(app: *mut u8);
}
