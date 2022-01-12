use core::mem::size_of;

#[track_caller]
fn assert_valid_page_address(page_address: u32) {
    assert!(
        page_address % 0x0000_1000 == 0,
        "Page addresses must be aligned to 4KB blocks"
    );
    assert!(
        page_address < 0x0010_0000,
        "Page cannot lie outside of flash memory"
    );
}

pub fn erase_page(page_address: u32, flash: &embassy_nrf::pac::nvmc::RegisterBlock) {
    assert_valid_page_address(page_address);

    // Enable the erase functionality of the flash
    flash.config.modify(|_, w| w.wen().een());
    // Start the erase process by writing a u32 word containing all 1's to the first word of the page
    // This is safe because the flash slice is page aligned, so a pointer to the first byte is valid as a pointer to a u32.
    unsafe {
        let first_word = page_address as *mut u32;
        first_word.write_volatile(0xFFFFFFFF);
    }
    // Wait for the erase to be done
    while flash.ready.read().ready().is_busy() {}

    flash.config.modify(|_, w| w.wen().ren());

    // Synchronize the changes
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
}

#[track_caller]
pub fn program_page(
    page_address: u32,
    data: &[u32],
    flash: &embassy_nrf::pac::nvmc::RegisterBlock,
) {
    assert_valid_page_address(page_address);
    assert!(
        data.len() <= 0x0000_1000 / size_of::<u32>(),
        "Only 4KB can be programmed at a time",
    );

    // Now we need to write the buffer to flash
    // Set the flash to write mode
    flash.config.modify(|_, w| w.wen().wen());

    // Write the buffer words to the flash
    for (data_word, flash_word) in data
        .iter()
        // Every word of the buffer corresponds to a word in flash
        .zip((page_address..page_address + 0x0000_1000).step_by(core::mem::size_of::<u32>()))
        // We only have to write when the words are different
        .filter(|(b, f)| *b != f)
    {
        unsafe {
            (flash_word as *mut u32).write_volatile(*data_word);
        }
        // Wait for the write to be done
        while flash.ready.read().ready().is_busy() {}
    }

    // Set the flash to default readonly mode
    flash.config.modify(|_, w| w.wen().ren());

    // Synchronize the changes
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
}
