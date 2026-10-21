/* Pokémon Bank 1.5, code 2dce4796...40abf only: the offline patch.
   - The start-up hook goes into the zero bytes between the end of the
     original text (0x00313910) and the end of its last page (0x00314000).
   - The payload goes on the first page after the original zero-initialised
     data (0x003FA904), allocated by the paired exheader.
   - The table in .patch.link_roots keeps the entry functions in the link
     (src/link.rs). */
OUTPUT_FORMAT(elf32-littlearm)
OUTPUT_ARCH(arm)
ENTRY(bank_offline_next)

PHDRS {
    bootstrap PT_LOAD FLAGS(5);
    payload_rx PT_LOAD FLAGS(5);
    payload_rw PT_LOAD FLAGS(6);
}

__patch_original_app_init = 0x0010494c;

SECTIONS {
    . = 0x00313910;
    .patch.bootstrap : {
        KEEP(*(.patch.bootstrap))
        KEEP(*(.patch.bootstrap.*))
    } :bootstrap
    ASSERT(SIZEOF(.patch.bootstrap) > 0, "missing bootstrap")
    ASSERT(SIZEOF(.patch.bootstrap) <= 0x6f0, "bootstrap exceeds the zero bytes after the original text")

    . = 0x003fb000;
    __patch_payload_start = .;
    .text : {
        FILL(0)
        *(.text .text.*)
        *(.rodata .rodata.*)
    } :payload_rx
    .ARM.extab : { *(.ARM.extab .ARM.extab.*) } :payload_rx
    .ARM.exidx : { *(.ARM.exidx .ARM.exidx.*) } :payload_rx
    .rx_padding : {
        FILL(0)
        . = ALIGN(0x1000);
    } :payload_rx
    __patch_payload_rx_end = .;
    __patch_payload_rx_size = __patch_payload_rx_end - __patch_payload_start;

    .data : {
        KEEP(*(.patch.link_roots))
        *(.data .data.*)
        *(.got .got.*)
    } :payload_rw
    .bss (NOLOAD) : {
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(0x1000);
    } :payload_rw
    __patch_payload_end = .;

    ASSERT(__patch_payload_rx_size > 0, "empty executable payload")
    ASSERT((__patch_payload_rx_end & 0xfff) == 0, "unaligned RX/RW boundary")
    ASSERT(__patch_payload_end < 0x08000000, "payload reaches application heap")
    /DISCARD/ : { *(.comment .note .note.* .eh_frame .eh_frame_hdr) }
}
