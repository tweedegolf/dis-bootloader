#![doc = include_str!("../../README.md")]
#![no_std]
#![warn(missing_docs)]

#[cfg(not(feature = "std-compat"))]
mod linker_flash_addresses;
#[cfg(not(feature = "std-compat"))]
/// Helper functions for finding the flash addresses of the memory regions more easily
pub mod flash_addresses {
    pub use crate::linker_flash_addresses::*;
}

#[cfg(feature = "std-compat")]
mod std_compat_flash_addresses;
#[cfg(feature = "std-compat")]
/// Helper functions for finding the flash addresses of the memory regions more easily
pub mod flash_addresses {
    pub use crate::std_compat_flash_addresses::*;
}

pub mod state;

/// A trait defining the common flash operations
pub trait Flash {
    /// Erase the given page
    fn erase_page(&mut self, page_address: u32);

    /// Program the page with the given data.
    /// Only the data words that are different from what is currently stored in flash may be written to.
    fn program_page(&mut self, page_address: u32, data: &[u32]);
}
