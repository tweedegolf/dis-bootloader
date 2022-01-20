use core::ops::Range;

extern "C" {
    static mut _bootloader_flash_start: u32;
    static mut _bootloader_flash_end: u32;
    static mut _bootloader_scratch_start: u32;
    static mut _bootloader_scratch_end: u32;
    static mut _bootloader_state_start: u32;
    static mut _bootloader_state_end: u32;

    static mut _program_slot_a_start: u32;
    static mut _program_slot_a_end: u32;
    static mut _program_slot_b_start: u32;
    static mut _program_slot_b_end: u32;
}

/// The size of a page in bytes
pub const PAGE_SIZE: u32 = 0x0000_1000;

pub fn bootloader_flash_range() -> Range<u32> {
    unsafe {
        let start = &_bootloader_flash_start as *const u32 as u32;
        let end = &_bootloader_flash_end as *const u32 as u32;
        start..end
    }
}

pub fn bootloader_flash_page_range() -> Range<u32> {
    let address_range = bootloader_flash_range();
    address_range.start / PAGE_SIZE..address_range.end / PAGE_SIZE
}

pub fn bootloader_scratch_range() -> Range<u32> {
    unsafe {
        let start = &_bootloader_scratch_start as *const u32 as u32;
        let end = &_bootloader_scratch_end as *const u32 as u32;
        start..end
    }
}

pub fn bootloader_scratch_page_range() -> Range<u32> {
    let address_range = bootloader_scratch_range();
    address_range.start / PAGE_SIZE..address_range.end / PAGE_SIZE
}

pub fn bootloader_state_range() -> Range<u32> {
    unsafe {
        let start = &_bootloader_state_start as *const u32 as u32;
        let end = &_bootloader_state_end as *const u32 as u32;
        start..end
    }
}

pub fn bootloader_state_page_range() -> Range<u32> {
    let address_range = bootloader_state_range();
    address_range.start / PAGE_SIZE..address_range.end / PAGE_SIZE
}

pub fn program_slot_a_range() -> Range<u32> {
    unsafe {
        let start = &_program_slot_a_start as *const u32 as u32;
        let end = &_program_slot_a_end as *const u32 as u32;
        start..end
    }
}

pub fn program_slot_a_page_range() -> Range<u32> {
    let address_range = program_slot_a_range();
    address_range.start / PAGE_SIZE..address_range.end / PAGE_SIZE
}

pub fn program_slot_b_range() -> Range<u32> {
    unsafe {
        let start = &_program_slot_b_start as *const u32 as u32;
        let end = &_program_slot_b_end as *const u32 as u32;
        start..end
    }
}

pub fn program_slot_b_page_range() -> Range<u32> {
    let address_range = program_slot_b_range();
    address_range.start / PAGE_SIZE..address_range.end / PAGE_SIZE
}
