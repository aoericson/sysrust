/*
 * linker.ld -- Kernel linker script.
 *
 * Defines the memory layout of the kernel binary:
 *   - Entry point is _start (in boot.s)
 *   - Kernel is loaded at 1MB (0x100000), the Multiboot convention
 *   - .multiboot comes first so QEMU finds the header in the first 8KB
 *   - Each section is page-aligned (4096 bytes) for paging support
 */
ENTRY(_start)

PHDRS
{
    text PT_LOAD FLAGS(5);   /* r-x : code is readable + executable */
    rodata PT_LOAD FLAGS(4); /* r-- : constants are read-only */
    data PT_LOAD FLAGS(6);   /* rw- : data is readable + writable */
}

SECTIONS
{
    . = 0x100000;            /* start at 1MB */

    /* Multiboot header — must be within the first 8KB of the binary */
    .multiboot ALIGN(4) : AT(ADDR(.multiboot))
    {
        KEEP(*(.multiboot))
    } :text

    /* Executable code */
    .text ALIGN(4096) : AT(ADDR(.text))
    {
        *(.text)
        *(.text.*)
    } :text

    /* Read-only data (string literals, const arrays) */
    .rodata ALIGN(4096) : AT(ADDR(.rodata))
    {
        *(.rodata)
        *(.rodata.*)
    } :rodata

    /* Initialized read-write data */
    .data ALIGN(4096) : AT(ADDR(.data))
    {
        *(.data)
        *(.data.*)
        *(.got)
        *(.got.plt)
    } :data

    /* Zero-initialized data (BSS) */
    .bss ALIGN(4096) : AT(ADDR(.bss))
    {
        __bss_start = .;
        *(.bss)
        *(.bss.*)
        *(COMMON)
        __bss_end = .;
    } :data

    /DISCARD/ :
    {
        *(.eh_frame)
        *(.comment)
    }
}
