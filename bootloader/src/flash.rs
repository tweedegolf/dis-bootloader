//! Implementation of [Flash]

use core::mem::size_of;

/// The bootloader's implementation of the flash operations
pub struct Flash<'a> {
    pub registers: &'a embassy_nrf::pac::nvmc::RegisterBlock,
}

impl<'a> shared::Flash for Flash<'a> {
    #[track_caller]
    fn erase_page(&mut self, page_address: u32) {
        assert_valid_page_address(page_address);

        // Enable the erase functionality of the flash
        self.registers.config.modify(|_, w| w.wen().een());
        // Start the erase process by writing a u32 word containing all 1's to the first word of the page
        // This is safe because the flash slice is page aligned, so a pointer to the first byte is valid as a pointer to a u32.
        unsafe {
            let first_word = page_address as *mut u32;
            first_word.write_volatile(0xFFFFFFFF);
        }
        // Wait for the erase to be done
        while self.registers.ready.read().ready().is_busy() {}

        self.registers.config.modify(|_, w| w.wen().ren());

        // Synchronize the changes
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
    }

    #[track_caller]
    fn program_page(&mut self, page_address: u32, data: &[u32]) {
        assert_valid_page_address(page_address);
        assert!(
            data.len() <= 0x0000_1000 / size_of::<u32>(),
            "Only 4KB can be programmed at a time",
        );

        // Now we need to write the buffer to flash
        // Set the flash to write mode
        self.registers.config.modify(|_, w| w.wen().wen());

        // Write the buffer words to the flash
        let word_size = core::mem::size_of::<u32>();
        let page_words = (page_address..page_address + 0x0000_1000)
            .step_by(word_size)
            .map(|address| address as *mut u32);

        // Every word of the buffer corresponds to a word in flash
        // We only have to write when the words are different
        for (data_word, flash_word_ptr) in data
            .iter()
            .zip(page_words)
            .filter(|(word, ptr)| **word != unsafe { **ptr })
        {
            unsafe {
                flash_word_ptr.write_volatile(*data_word);
            }
            // Wait for the write to be done
            while self.registers.ready.read().ready().is_busy() {}
        }

        // Set the flash to default readonly mode
        self.registers.config.modify(|_, w| w.wen().ren());

        // Synchronize the changes
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
    }

    fn read<I: core::slice::SliceIndex<[u8]>>(&self, address_range: I) -> &I::Output {
        let entire_flash_slice = unsafe {
            core::slice::from_raw_parts(
                0x0000_0000 as *const u8,
                0x0010_0000,
            )
        };

        entire_flash_slice.get(address_range).unwrap()
    }

}

/// Asserts that the address is at the start of a flash page
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
