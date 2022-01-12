use core::{mem::size_of, ops::Range};
use num_enum::{IntoPrimitive, TryFromPrimitive};

use crate::{
    flash::{erase_page, program_page},
    flash_addresses::{bootloader_state_range, program_slot_a_page_range},
};

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
    const FINISHED_PAGE_RANGE: Range<usize> = 768..1024;

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

    pub fn get_page_state(&self, page: u32) -> PageState {
        let cached_value = self.buffer[Self::CACHED_PAGES_RANGE][page as usize];
        let copied_value = self.buffer[Self::COPIED_PAGES_RANGE][page as usize];
        let finished_value = self.buffer[Self::FINISHED_PAGE_RANGE][page as usize];

        match (cached_value, copied_value, finished_value) {
            (0xFFFF_FFFF, 0xFFFF_FFFF, 0xFFFF_FFFF) => PageState::Original,
            (scratch_page, 0xFFFF_FFFF, 0xFFFF_FFFF) => PageState::InScratch { scratch_page },
            (scratch_page, Self::VALID_WORD, 0xFFFF_FFFF) => {
                PageState::InScratchOverwritten { scratch_page }
            }
            (_, _, Self::VALID_WORD) => PageState::Swapped,
            p => unreachable!("Invalid page state: {:X?}", p),
        }
    }

    pub fn set_page_state(&mut self, page: u32, state: PageState) {
        let (cached_value, copied_value, finished_value) = match state {
            PageState::Original => (0xFFFF_FFFF, 0xFFFF_FFFF, 0xFFFF_FFFF),
            PageState::InScratch { scratch_page } => (scratch_page, 0xFFFF_FFFF, 0xFFFF_FFFF),
            PageState::InScratchOverwritten { scratch_page } => {
                (scratch_page, Self::VALID_WORD, 0xFFFF_FFFF)
            }
            PageState::Swapped => (Self::VALID_WORD, Self::VALID_WORD, Self::VALID_WORD),
        };

        self.buffer[Self::CACHED_PAGES_RANGE][page as usize] = cached_value;
        self.buffer[Self::COPIED_PAGES_RANGE][page as usize] = copied_value;
        self.buffer[Self::FINISHED_PAGE_RANGE][page as usize] = finished_value;
    }

    /// Sets the state so that a swap can be started.
    /// Also performs a fresh erase so that all expected burn-in flashing can happen as expected.
    pub fn prepare_swap(&mut self, test_swap: bool, flash: &embassy_nrf::pac::nvmc::RegisterBlock) {
        // We're starting a swap, so our new goal is finishing it
        self.set_goal(if test_swap {
            BootloaderGoal::FinishTestSwap
        } else {
            BootloaderGoal::FinishSwap
        });

        for page in 0..program_slot_a_page_range().len() as u32 {
            self.set_page_state(page, PageState::Original);
        }

        self.store(flash);
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

    /// Stores the bootloader buffer in flash by first erasing the flash and then performing a burn-store
    pub fn store(&self, flash: &embassy_nrf::pac::nvmc::RegisterBlock) {
        // Erase the page
        erase_page(bootloader_state_range().start, flash);

        // Store the buffer
        self.burn_store(flash);
    }

    /// Stores the bootloader buffer in flash, but does not perform an erase and
    /// only emits word write for words that have changes in them.
    /// Every word may be written to twice.
    /// The burn store can only change bits from 1 to 0.
    pub fn burn_store(&self, flash: &embassy_nrf::pac::nvmc::RegisterBlock) {
        program_page(bootloader_state_range().start, &self.buffer, flash);
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
    #[num_enum(alternatives = [0xFFFF_FFFF])]
    JumpToApplication = 0,
    /// The B image should be swapped into the A image slot
    StartSwap = 1,
    /// (Internal state only) The bootloader started swapping and should finish it.
    /// This is only ever relevant when the bootloader was reset in the middle of a swap.
    FinishSwap = 2,
    /// The B image should be swapped into the A image slot. After than, this state is set to [StartSwap] again
    /// to let the bootloader swap back the image after another reboot. This is similar to the MCUboot test swap.
    /// The application can verify itself by setting the goal to [JumpToApplication] to prevent rollback.
    StartTestSwap = 3,
    /// (Internal state only) The bootloader started test swapping and should finish it.
    /// This is only ever relevant when the bootloader was reset in the middle of a test swap.
    FinishTestSwap = 4,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum PageState {
    /// This page is still in the original spot
    Original,
    /// The A page has been copied into the scratch area
    InScratch {
        /// The page of the scratch area the A page has been copied into
        scratch_page: u32,
    },
    /// The A page has been copied into the scratch area and the B page has overwritten the original A page
    InScratchOverwritten {
        /// The page of the scratch area the A page has been copied into
        scratch_page: u32,
    },
    /// The scratch page containing the original A page has been written to the B page spot.
    /// The swap is thus done.
    Swapped,
}

impl PageState {
    /// Returns `true` if the page state is [`Swapped`].
    ///
    /// [`Swapped`]: PageState::Swapped
    pub fn is_swapped(&self) -> bool {
        matches!(self, Self::Swapped)
    }
}
