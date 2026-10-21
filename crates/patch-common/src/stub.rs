//! Entries written in assembly: where a patched site of the original needs
//! its registers kept or handed on in a way a Rust function cannot. Each is
//! one ARM function `<name>` in its own section `.text.<name>`, with its
//! literal pool, and is declared as an `extern` function for the link roots
//! (`link`). Its name ends in `_stub`.
//!
//! `message_stub!` is the entry for the original's "show a message"
//! function, the same engine code in Bank and Transporter: `(ui, ...,
//! wait)` beginning with `push {r4-r8, lr}`, which a patch replaces by a
//! branch to it. The patches refuse HOME and sleep by the task that comes
//! next, for as long as it reads or writes. A message that waits is where
//! such a task has nothing left to do but end, so the refusal ends there.

/// Defines the entry `$name` from its instructions and their operands.
#[macro_export]
macro_rules! stub {
    ($name:ident, [$($line:expr),* $(,)?] $(, $($operand:tt)*)?) => {
        core::arch::global_asm!(
            ".syntax unified",
            ".arm",
            concat!(".section .text.", stringify!($name), ",\"ax\",%progbits"),
            ".balign 4",
            concat!(".global ", stringify!($name)),
            concat!(".type ", stringify!($name), ", %function"),
            concat!(stringify!($name), ":"),
            $($line,)*
            ".ltorg",
            concat!(".size ", stringify!($name), ", .-", stringify!($name)),
            $($($operand)*)?
        );
        unsafe extern "aapcs" {
            /// # Safety
            /// Only the patched site of the original it is made for may
            /// enter it, with the registers that site has.
            pub fn $name();
        }
    };
}

/// Defines the entry `$name` for a "show a message" whose `wait` argument is
/// in register `$wait` and whose second instruction is at `$resume`. It runs
/// the replaced instruction, calls `$waits()` for a message that waits, and
/// goes on inside the original with every register as it came.
#[macro_export]
macro_rules! message_stub {
    ($name:ident, $wait:literal, $resume:literal, $waits:path) => {
        $crate::stub!(
            $name,
            [
                "push {{r4, r5, r6, r7, r8, lr}}",
                concat!("cmp ", $wait, ", #0"),
                "beq 1f",
                "push {{r0-r3, r12, lr}}",
                "bl {waits}",
                "pop {{r0-r3, r12, lr}}",
                "1:",
                concat!("ldr r12, =", $resume),
                "bx r12",
            ],
            waits = sym $waits,
        );
    };
}
