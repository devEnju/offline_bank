/* Reviewed Bank code 2dce4796...40abf only. No native hook is installed here.
   The migration patch: the same placement as patches/bank/link/bank15.ld,
   with its own two entry functions. */
OUTPUT_FORMAT(elf32-littlearm)
OUTPUT_ARCH(arm)
ENTRY(bank_migrate_next)

PHDRS {
    bootstrap PT_LOAD FLAGS(5);
    payload_rx PT_LOAD FLAGS(5);
    payload_rw PT_LOAD FLAGS(6);
}

__bank_original_app_init = 0x0010494c;

SECTIONS {
    . = 0x00313910;
    .bank.bootstrap : {
        KEEP(*(.bank.bootstrap))
    } :bootstrap
    ASSERT(SIZEOF(.bank.bootstrap) > 0, "missing bootstrap")
    ASSERT(SIZEOF(.bank.bootstrap) <= 0x6f0, "bootstrap exceeds reviewed text padding")

    . = 0x003fb000;
    __bank_payload_start = .;
    .text : {
        FILL(0)
        KEEP(*(.text.bank_migrate_next))
        KEEP(*(.text.bank_migrate_open))
        *(.text .text.*)
        *(.rodata .rodata.*)
    } :payload_rx
    .ARM.extab : { *(.ARM.extab .ARM.extab.*) } :payload_rx
    .ARM.exidx : { *(.ARM.exidx .ARM.exidx.*) } :payload_rx
    .rx_padding : {
        FILL(0)
        . = ALIGN(0x1000);
    } :payload_rx
    __bank_payload_rx_end = .;
    __bank_payload_rx_size = __bank_payload_rx_end - __bank_payload_start;

    .data : {
        KEEP(*(.bank.link_roots))
        *(.data .data.*)
        *(.got .got.*)
    } :payload_rw
    .bss (NOLOAD) : {
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(0x1000);
    } :payload_rw
    __bank_payload_end = .;

    ASSERT(__bank_payload_rx_size > 0, "empty executable payload")
    ASSERT((__bank_payload_rx_end & 0xfff) == 0, "unaligned RX/RW boundary")
    ASSERT(__bank_payload_end < 0x08000000, "payload reaches application heap")
    /DISCARD/ : { *(.comment .note .note.* .eh_frame .eh_frame_hdr) }
}
