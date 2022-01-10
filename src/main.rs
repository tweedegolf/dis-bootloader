#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]

use crate::flash_addresses::*;
use flash::{erase_page, program_page};
use rtt_target::{rprintln, rtt_init_print};
use state::BootloaderState;

mod flash;
mod flash_addresses;
mod state;

#[cortex_m_rt::entry]
fn main() -> ! {
    let _cp = cortex_m::Peripherals::take().unwrap();
    let mut dp = nrf9160_pac::Peripherals::take().unwrap();

    rtt_init_print!(NoBlockSkip, 32384);

    rprintln!("Starting bootloader");
    rprintln!("Defined memory regions:");
    rprintln!(
        "\tbootloader flash:   {:08X?} ({:03?})",
        bootloader_flash_range(),
        bootloader_flash_page_range()
    );
    rprintln!(
        "\tbootloader scratch: {:08X?} ({:03?})",
        bootloader_scratch_range(),
        bootloader_scratch_page_range()
    );
    rprintln!(
        "\tbootloader state:   {:08X?} ({:03?})",
        bootloader_state_range(),
        bootloader_state_page_range()
    );
    rprintln!(
        "\tprogram slot a:     {:08X?} ({:03?})",
        program_slot_a_range(),
        program_slot_a_page_range()
    );
    rprintln!(
        "\tprogram slot b:     {:08X?} ({:03?})",
        program_slot_b_range(),
        program_slot_b_page_range()
    );

    // Let's check what we need to do by loading the state
    let mut state = BootloaderState::load();

    // The state must be valid or we will just jump to the application
    if !state.is_valid() {
        rprintln!("State is invalid, jumping to application");
        jump_to_application();
    }

    let goal = state.goal();
    rprintln!("Goal: {:?}", goal);

    match goal {
        state::BootloaderGoal::JumpToApplication => {
            jump_to_application();
        }
        state::BootloaderGoal::StartSwap => {
            state.prepare_swap(false, &mut dp.NVMC_S);
            perform_swap(false, &mut state, &mut dp.NVMC_S);
        }
        state::BootloaderGoal::FinishSwap => perform_swap(false, &mut state, &mut dp.NVMC_S),
        state::BootloaderGoal::StartTestSwap => {
            state.prepare_swap(true, &mut dp.NVMC_S);
            perform_swap(true, &mut state, &mut dp.NVMC_S);
        }
        state::BootloaderGoal::FinishTestSwap => perform_swap(true, &mut state, &mut dp.NVMC_S),
    }

    loop {}
}

fn perform_swap(test_swap: bool, state: &mut BootloaderState, flash: &mut nrf9160_pac::NVMC_S) {
    let total_program_pages = program_slot_a_page_range().len() as u32;
    let total_scratch_pages = bootloader_scratch_page_range().len() as u32;

    let mut scratch_page_index = 0;

    for page in 0..total_program_pages {
        let slot_a_page = program_slot_a_page_range().start + page;
        let slot_a_address = slot_a_page * PAGE_SIZE;
        let slot_b_page = program_slot_b_page_range().start + page;
        let slot_b_address = slot_b_page * PAGE_SIZE;

        while !state.get_page_state(page).is_swapped() {
            rprintln!("Swapping page {}: {:?}", page, state.get_page_state(page));
            match state.get_page_state(page) {
                state::PageState::Original => {
                    // We need to copy the A page to a scratch page

                    // Decide which scratch page to use
                    let scratch_page = bootloader_scratch_page_range().start + scratch_page_index;
                    let scratch_address = scratch_page * PAGE_SIZE;
                    // Erase the scratch area
                    erase_page(scratch_address, flash);
                    // Program the data from slot A into the scratch slot
                    program_page(
                        scratch_address,
                        unsafe {
                            core::slice::from_raw_parts(
                                slot_a_address as *const u32,
                                PAGE_SIZE as usize,
                            )
                        },
                        flash,
                    );
                    // Update the state
                    state.set_page_state(page, state::PageState::InScratch { scratch_page });
                    state.burn_store(flash);
                }
                state::PageState::InScratch { scratch_page } => {
                    // We need to copy the B page to the A slot

                    // Erase the A page
                    erase_page(slot_a_address, flash);
                    // Program the data from slot B into the A slot
                    program_page(
                        slot_a_address,
                        unsafe {
                            core::slice::from_raw_parts(
                                slot_b_address as *const u32,
                                PAGE_SIZE as usize,
                            )
                        },
                        flash,
                    );
                    // Update the state
                    state.set_page_state(
                        page,
                        state::PageState::InScratchOverwritten { scratch_page },
                    );
                    state.burn_store(flash);
                }
                state::PageState::InScratchOverwritten { scratch_page } => {
                    // We need to copy the scratch page to the B slot

                    let scratch_address = scratch_page * PAGE_SIZE;

                    // Erase the B page
                    erase_page(slot_b_address, flash);
                    // Program the data from the scratch slot into the B slot
                    program_page(
                        slot_b_address,
                        unsafe {
                            core::slice::from_raw_parts(
                                scratch_address as *const u32,
                                PAGE_SIZE as usize,
                            )
                        },
                        flash,
                    );
                    // Update the state
                    state.set_page_state(page, state::PageState::Swapped);

                    state.burn_store(flash);
                }
                state::PageState::Swapped => {
                    // We're done and shouldn't be able to get here
                    unreachable!()
                }
            }
        }

        scratch_page_index = (scratch_page_index + 1) % total_scratch_pages;
    }

    // We're done, so we should change the state
    if test_swap {
        state.set_goal(state::BootloaderGoal::StartSwap);
    } else {
        state.set_goal(state::BootloaderGoal::JumpToApplication);
    }
}

fn jump_to_application() -> ! {
    let application_address = program_slot_a_range().start + 0x200;

    rprintln!("Jumping to {:#08X}", application_address);

    unsafe {
        cortex_m::asm::bootload(application_address as *const u32)
    }
}

#[cortex_m_rt::exception]
unsafe fn HardFault(frame: &cortex_m_rt::ExceptionFrame) -> ! {
    panic!("{:?}", frame);
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    rprintln!("{}", info);
    loop {
        cortex_m::asm::bkpt();
    }
}
