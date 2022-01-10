use core::{ops::{Range, Index}, mem::size_of};
use bitvec::{view::{BitView, AsBits}, field::BitField, order::Lsb0, slice::{BitSlice, BitSliceIndex}};
use num_enum::{IntoPrimitive, TryFromPrimitive};

use crate::flash_addresses::bootloader_state_range;

/// This state is stored on the state page
pub struct BootloaderState {
    buffer: [u32; 4096 / size_of::<u32>()],
}

impl BootloaderState {
    /// The word that needs to be present to know if the state is valid instead of erased or random bits
    const VALID_WORD: u32 = 0xB00210AD; // Bootload
    /// The index of where the Valid word is stored
    const VALID_WORD_INDEX: usize = 0;
    /// The index of where the goal is stored
    const GOAL_INDEX: usize = 1;
    
    /// The range of words that stores the page status for the copy from the A image to scratch
    const CACHED_PAGES_RANGE: Range<usize> = 256..512;
    /// The range of words that stores the page status for the copy from the B image to the A image
    const COPIED_PAGES_RANGE: Range<usize> = 512..768;
    /// The range of words that stores the page status for the copy from scratch to the B image
    const FINISHED_PAGES_STATUS_RANGE: Range<usize> = 768..1024;

    pub fn is_valid(&self) -> bool {
        self.buffer[Self::VALID_WORD_INDEX] == Self::VALID_WORD
    }

    pub fn set_valid(&mut self, validity: bool) {
        let value = if validity {
            Self::VALID_WORD
        } else {
            0xFFFF_FFFF
        };

        self.buffer[Self::GOAL_INDEX] = value;
    }

    /// Get the stored goal value from the buffer.
    /// Panics if the goal is in an invalid state.
    pub fn goal(&self) -> BootloaderGoal {
        self.buffer[Self::GOAL_INDEX].try_into().unwrap()
    }

    /// Sets the stored goal value into the buffer.
    pub fn set_goal(&mut self, goal: BootloaderGoal) {
        self.buffer[Self::GOAL_INDEX] = goal.into();
    }

    /// Loads the bootloader state from flash
    pub fn load() -> Self {
        // Get where the state is stored
        let state_flash_slice = unsafe { Self::get_flash_slice() };

        // Create our buffer and do a sanity check
        let mut buffer = [0xFFFFFFFF; 1024];

        // Read the flash into our ram buffer
        buffer.copy_from_slice(state_flash_slice);

        Self { buffer }
    }

    /// Stores the bootloader buffer in flash
    pub fn store(&self, flash: &mut nrf9160_pac::NVMC_S) {
        // Get the flash slice.
        // This is safe, because there's no other reference to it.
        let flash_slice = unsafe { Self::get_flash_slice_mut() };

        // Enable the erase functionality of the flash
        flash.config.modify(|_, w| w.wen().een());
        // Start the erase process by writing a u32 word containing all 1's to the first word of the page
        // This is safe because the flash slice is page aligned, so a pointer to the first byte is valid as a pointer to a u32.
        unsafe {
            let first_word = flash_slice.as_mut_ptr() as *mut u32;
            first_word.write_volatile(0xFFFFFFFF);
        }
        // Wait for the erase to be done
        while flash.ready.read().ready().is_busy() {}

        // Synchronize the changes
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
        
        // Store the buffer
        self.burn_store(flash);
    }

    /// Stores the bootloader buffer in flash, but does not perform an erase and
    /// only emits word write for words that have changes in them.
    /// Every word may be written to twice.
    /// The burn store can only change bits from 1 to 0.
    pub fn burn_store(&self, flash: &mut nrf9160_pac::NVMC_S) {
        // Get the flash slice.
        // This is safe, because there's no other reference to it.
        let flash_slice = unsafe { Self::get_flash_slice_mut() };

        // Now we need to write the buffer to flash
        // Set the flash to write mode
        flash.config.modify(|_, w| w.wen().wen());

        // Write the buffer words to the flash
        for (buffer_word, flash_word) in self
            .buffer
            .iter()
            // Every word of the buffer corresponds to a word in flash
            .zip(flash_slice.iter_mut())
            // We only have to write when the words are different
            .filter(|(b, f)| b != f)
        {
            let flash_word_ptr: *mut u32 = flash_word;
            unsafe {
                flash_word_ptr.write_volatile(*buffer_word);
            }
            // Wait for the write to be done
            while flash.readynext.read().readynext().is_busy() {}
        }

        // Wait for the write to be fully done
        while flash.ready.read().ready().is_busy() {}

        // Set the flash to default readonly mode
        flash.config.modify(|_, w| w.wen().ren());

        // Synchronize the changes
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
    }

    unsafe fn get_flash_slice_mut() -> &'static mut [u32] {
        let state_range = bootloader_state_range();
        let start_ptr = state_range.start as *mut u32;
        core::slice::from_raw_parts_mut(start_ptr, state_range.len() / size_of::<u32>())
    }

    unsafe fn get_flash_slice() -> &'static [u32] {
        let state_range = bootloader_state_range();
        let start_ptr = state_range.start as *const u32;
        core::slice::from_raw_parts(start_ptr, state_range.len() / size_of::<u32>())
    }
}

#[repr(u32)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, IntoPrimitive, TryFromPrimitive)]
pub enum BootloaderGoal {
    /// The bootloader should do nothing and just jump to the application
    JumpToApplication = 0xFFFF_FFFF,
    /// The B image should be swapped into the A image slot
    StartSwap = 0,
    /// (Internal state only) The bootloader started swapping and should finish it.
    /// This is only ever relevant when the bootloader was reset in the middle of a swap.
    FinishSwap = 1,
    /// The B image should be swapped into the A image slot. After than, this state is set to [StartSwap] again
    /// to let the bootloader swap back the image after another reboot. This is similar to the MCUboot test swap.
    /// The application can verify itself by setting the goal to [JumpToApplication] to prevent rollback.
    StartTestSwap = 2,
    /// (Internal state only) The bootloader started test swapping and should finish it.
    /// This is only ever relevant when the bootloader was reset in the middle of a test swap.
    FinishTestSwap = 3,
}
