//! Implementation of the bootloader state

use crate::{
    flash_addresses::{bootloader_state_range, program_slot_a_page_range, PAGE_SIZE},
    Flash,
};
use core::{mem::size_of, ops::Range};
use num_enum::{IntoPrimitive, TryFromPrimitive};

/// This state is stored on the state pages.
///
/// It is both the API the application uses to set the bootloader goal and the store for the swapping process.
///
/// Semantically this is stored on one flash page, but if it were only stored on one, then
/// there is a possibility that the page would be corrupted in the erase-program cycle.
/// By using two pages, this is prevented.
pub struct BootloaderState {
    buffer: [u32; 4096 / size_of::<u32>()],
}

impl BootloaderState {
    /// The word that needs to be present to know if the state is valid instead of erased or random bits
    const VALID_WORD: u32 = 0xB00210AD; // Bootload

    /// The index of where the crc is stored
    const CRC_INDEX: usize = 0;
    /// The index of where the goal is stored
    const GOAL_INDEX: usize = 1;

    /// The range of words that stores the page status for the copy from the A image to scratch
    const CACHED_PAGES_RANGE: Range<usize> = 256..512;
    /// The range of words that stores the page status for the copy from the B image to the A image
    const COPIED_PAGES_RANGE: Range<usize> = 512..768;
    /// The range of words that stores the page status for the copy from scratch to the B image
    const FINISHED_PAGE_RANGE: Range<usize> = 768..1024;

    /// Tests if the state is valid by running a CRC over it and comparing the result against the stored CRC
    pub fn is_valid(&self) -> bool {
        let stored_crc = self.buffer[Self::CRC_INDEX];
        let calculated_crc = self.calculate_self_crc();
        stored_crc == calculated_crc
    }

    /// If set to true, calculates the CRC of the current state and sets the crc word to the result.
    /// If set to false, the crc word is set to a default wrong value.
    pub fn set_valid(&mut self, validity: bool) {
        let crc_value = if validity {
            self.calculate_self_crc()
        } else {
            0xFFFF_FFFF
        };

        self.buffer[Self::CRC_INDEX] = crc_value;
    }

    /// Calculates the crc of the internal buffer between the crc word and the page states.
    /// The crc is not included because we can't calculate that.
    /// The page state ranges are not included because those are burn_stored and we don't want to have to update the CRC
    /// everytime because that would defeat the purpose of doing the burn stores.
    fn calculate_self_crc(&self) -> u32 {
        let crc = crc::Crc::<u32>::new(&crc::CRC_32_MPEG_2);
        let mut digest = crc.digest();
        for word in &self.buffer[Self::CRC_INDEX + 1..Self::CACHED_PAGES_RANGE.start] {
            digest.update(&word.to_ne_bytes());
        }
        digest.finalize()
    }

    /// Get the stored goal value from the buffer.
    /// Panics if the goal is in an invalid state.
    pub fn goal(&self) -> BootloaderGoal {
        self.buffer[Self::GOAL_INDEX].try_into().unwrap()
    }

    /// Sets the stored goal value into the buffer.
    pub fn set_goal(&mut self, goal: BootloaderGoal) {
        // When we change the goal, we also need to update the CRC
        let is_valid = self.is_valid();

        self.buffer[Self::GOAL_INDEX] = goal.into();

        if is_valid {
            // The state was valid before, so let's update it so it is valid again
            self.set_valid(is_valid);
        }
    }

    /// Gets the state of the page with the given index. The index is global,
    /// so the page that starts at address 0x000A_3000 has index 0xA3.
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

    /// Sets the page state to the given value.
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
    pub fn prepare_swap(&mut self, test_swap: bool, flash: &mut impl Flash) {
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
    pub fn load(flash: &impl Flash) -> Self {
        // Get where the state is stored
        let (state_flash_slice_0, state_flash_slice_1) = unsafe { Self::get_state_flash_slices(flash) };

        // Create our buffer and do a sanity check
        let mut buffer = [0xFFFFFFFF; 1024];

        // Read the flash into our ram buffer
        buffer.copy_from_slice(state_flash_slice_0);

        let mut s = Self { buffer };

        // If the first page is not valid (which is possible when the [Self::store] function gets reset inbetween or during its erase_page and program_page calls),
        // Then we want to return the second page.
        if !s.is_valid() {
            s.buffer.copy_from_slice(state_flash_slice_1);
        }

        s
    }

    /// Stores the bootloader buffer in flash by first erasing the flash and then performing a burn-store
    pub fn store(&self, flash: &mut impl Flash) {
        // Erase the first page
        flash.erase_page(bootloader_state_range().start);
        // Store the buffer in the first page
        flash.program_page(bootloader_state_range().start, &self.buffer);
        // Erase the second page
        flash.erase_page(bootloader_state_range().start + PAGE_SIZE);
        // Store the buffer in the second page
        flash.program_page(bootloader_state_range().start + PAGE_SIZE, &self.buffer);
    }

    /// Stores the bootloader buffer in flash, but does not perform an erase and
    /// only emits word write for words that have changes in them.
    /// Every word may be written to twice.
    /// The burn store can only change bits from 1 to 0.
    pub fn burn_store(&self, flash: &mut impl Flash) {
        flash.program_page(bootloader_state_range().start, &self.buffer);
        flash.program_page(bootloader_state_range().start + PAGE_SIZE, &self.buffer);
    }

    unsafe fn get_state_flash_slices<'flash>(flash: &'flash impl Flash) -> (&'flash [u32], &'flash [u32]) {
        flash.read_u32(bootloader_state_range()).split_at(1024)
    }
}

/// The goal of the bootloader
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
    /// The B image should be swapped into the A image slot. After than, this state is set to [Self::StartSwap] again
    /// to let the bootloader swap back the image after another reboot. This is similar to the MCUboot test swap.
    /// The application can verify itself by setting the goal to [Self::JumpToApplication] to prevent rollback.
    StartTestSwap = 3,
    /// (Internal state only) The bootloader started test swapping and should finish it.
    /// This is only ever relevant when the bootloader was reset in the middle of a test swap.
    FinishTestSwap = 4,
}

/// The state of a page
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
