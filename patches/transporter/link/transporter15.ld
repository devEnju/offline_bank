/* Poké Transporter 1.5, code 001c20ad...f0fd2f only: the offline patch.
   - The start-up hook goes into the zero bytes between the end of the
     original text (0x0028D1AC) and the end of its last page (0x0028E000).
   - The payload goes on the first page after the original zero-initialised
     data (0x003638A4), allocated by the paired exheader.
   - The table in .patch.link_roots keeps the entry functions in the link
     (src/link.rs). */
OUTPUT_FORMAT(elf32-littlearm)
OUTPUT_ARCH(arm)
ENTRY(transporter_next)

PHDRS {
    bootstrap PT_LOAD FLAGS(5);
    payload_rx PT_LOAD FLAGS(5);
    payload_rw PT_LOAD FLAGS(6);
}

__patch_original_app_init = 0x00104644;

SECTIONS {
    . = 0x0028D1AC;
    .patch.bootstrap : {
        KEEP(*(.patch.bootstrap))
        KEEP(*(.patch.bootstrap.*))
    } :bootstrap
    ASSERT(SIZEOF(.patch.bootstrap) > 0, "missing bootstrap")
    ASSERT(SIZEOF(.patch.bootstrap) <= 0xE54, "bootstrap exceeds the zero bytes after the original text")

    . = 0x00364000;
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
