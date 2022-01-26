#![doc = include_str!("../../README.md")]
#![no_std]
#![warn(missing_docs)]

pub mod flash_addresses;
pub mod state;

/// A trait defining the common flash operations
pub trait Flash {
    /// Erase the given page
    fn erase_page(&mut self, page_address: u32);

    /// Program the page with the given data.
    /// Only the data words that are different from what is currently stored in flash may be written to.
    fn program_page(&mut self, page_address: u32, data: &[u32]);
}
