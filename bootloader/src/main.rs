#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]

use core::mem::MaybeUninit;

use crate::flash::Flash;
use embassy_nrf::{
    gpio::NoPin,
    interrupt,
    peripherals::UARTETWISPI0,
    uarte::{self, Uarte},
};
use embassy_traits::uart::{Read, Write};
use panic_persist::get_panic_message_bytes;
use shared::{
    flash_addresses::{
        bootloader_flash_page_range, bootloader_flash_range, bootloader_scratch_page_range,
        bootloader_scratch_range, bootloader_state_page_range, bootloader_state_range,
        program_slot_a_page_range, program_slot_a_range, program_slot_b_page_range,
        program_slot_b_range, PAGE_SIZE,
    },
    state::{BootloaderGoal, BootloaderState, PageState},
};

mod flash;

type Uart = Uarte<'static, UARTETWISPI0>;

#[link_section = ".uninit"]
static mut PANIC_COUNTS: MaybeUninit<u32> = MaybeUninit::uninit();

#[embassy::main]
async fn main(_spawner: embassy::executor::Spawner, p: embassy_nrf::Peripherals) {
    run_main(p).await;
}

#[macro_export]
macro_rules! uprintln {
    ($uart:expr, $($arg:tt)*) => {
        {
            use core::fmt::Write as _;
            let mut str = arrayvec::ArrayString::<1024>::new();
            match writeln!(str, $($arg)*) {
                Ok(_) => {
                    $uart.write(str.as_bytes()).await.unwrap();
                },
                Err(_) => $uart.write("Error: failed to print string, too long".as_bytes()).await.unwrap(),
            };
        }
    };
}

async fn run_main(p: embassy_nrf::Peripherals) {
    // Embassy doesn't give us a pac instance of the NVMC, so we need to make a reference ourselves
    let mut flash = Flash {
        registers: unsafe { &*embassy_nrf::pac::NVMC::PTR },
    };

    let mut config = uarte::Config::default();
    config.parity = uarte::Parity::EXCLUDED;
    config.baudrate = uarte::Baudrate::BAUD115200;

    let irq = interrupt::take!(UARTE0_SPIM0_SPIS0_TWIM0_TWIS0);

    #[cfg(feature = "feather")]
    let (uart_rx_pin, uart_tx_pin) = (p.P0_05, p.P0_06);
    #[cfg(feature = "logistics")]
    let (uart_rx_pin, uart_tx_pin) = (p.P0_28, p.P0_29);
    #[cfg(feature = "mobility")]
    let (uart_rx_pin, uart_tx_pin) = (p.P0_28, p.P0_29);

    let mut uart: Uart = uarte::Uarte::new(
        p.UARTETWISPI0,
        irq,
        uart_rx_pin,
        uart_tx_pin,
        NoPin,
        NoPin,
        config,
    );

    uprintln!(
        uart,
        "\n\n--== == == == == == == == == == == == == == ==--\nStarting bootloader version `{}` with git hash `{}`",
        env!("CP_CARGO"),
        env!("CP_GIT")
    );
    let panics = unsafe { PANIC_COUNTS.assume_init_mut() };
    if *panics > 10 {
        // Probably random garbage from ram, so we've probably just booted
        *panics = 0;
    }

    // Check if there was a panic message, if so, send to UART
    if let Some(msg) = get_panic_message_bytes() {
        uprintln!(uart, "Booted up from a panic:");
        uart.write(msg).await.unwrap();
        *panics += 1;
        uprintln!(uart, "");
    }

    uprintln!(uart, "There have been {} panics so far.", panics);

    // If there are too many panics, let's just sleep and potentially save the flash memory
    if *panics > 10 {
        uprintln!(uart, "There have been too many panics. Bootloader will try to save the flash by going to sleep. The device can be woken up by sending a single byte over serial. The panics counter will then be reset to 0 so you can see all the output again");
        let mut buffer = [0; 1];
        uart.read(&mut buffer).await.unwrap();
        *panics = 0;
    }

    uprintln!(uart, "\nDefined memory regions:");
    uprintln!(
        uart,
        "\tbootloader flash:   {:08X?} ({:03?})",
        bootloader_flash_range(),
        bootloader_flash_page_range()
    );
    uprintln!(
        uart,
        "\tbootloader scratch: {:08X?} ({:03?})",
        bootloader_scratch_range(),
        bootloader_scratch_page_range()
    );
    uprintln!(
        uart,
        "\tbootloader state:   {:08X?} ({:03?})",
        bootloader_state_range(),
        bootloader_state_page_range()
    );
    uprintln!(
        uart,
        "\tprogram slot a:     {:08X?} ({:03?})",
        program_slot_a_range(),
        program_slot_a_page_range()
    );
    uprintln!(
        uart,
        "\tprogram slot b:     {:08X?} ({:03?})",
        program_slot_b_range(),
        program_slot_b_page_range()
    );

    // Let's check what we need to do by loading the state
    let mut state = BootloaderState::load();

    // The state must be valid or we will just jump to the application
    if !state.is_valid() {
        uprintln!(uart, "State is invalid, jumping to application");
        jump_to_application(uart).await;
    }

    let goal = state.goal();
    uprintln!(uart, "Goal: {:?}", goal);

    match goal {
        BootloaderGoal::JumpToApplication => {
            jump_to_application(uart).await;
        }
        BootloaderGoal::StartSwap => {
            state.prepare_swap(false, &mut flash); // TODO: think about reset here
            perform_swap(false, &mut state, &mut flash, &mut uart).await;
            jump_to_application(uart).await;
        }
        BootloaderGoal::FinishSwap => {
            perform_swap(false, &mut state, &mut flash, &mut uart).await;
            jump_to_application(uart).await;
        }
        BootloaderGoal::StartTestSwap => {
            state.prepare_swap(true, &mut flash);
            perform_swap(true, &mut state, &mut flash, &mut uart).await;
            jump_to_application(uart).await;
        }
        BootloaderGoal::FinishTestSwap => {
            perform_swap(true, &mut state, &mut flash, &mut uart).await;
            jump_to_application(uart).await;
        }
    }

    loop {}
}

async fn perform_swap(
    test_swap: bool,
    state: &mut BootloaderState,
    flash: &mut impl shared::Flash,
    uart: &mut Uart,
) {
    let total_program_pages = program_slot_a_page_range().len() as u32;
    let total_scratch_pages = bootloader_scratch_page_range().len() as u32;

    uprintln!(uart, "total_program_pages: {}", total_program_pages);
    uprintln!(uart, "total_scratch_pages: {}", total_scratch_pages);

    let mut scratch_page_index = 0;

    for page in 0..total_program_pages {
        let slot_a_page = program_slot_a_page_range().start + page;
        let slot_a_address = slot_a_page * PAGE_SIZE;
        let slot_b_page = program_slot_b_page_range().start + page;
        let slot_b_address = slot_b_page * PAGE_SIZE;

        while !state.get_page_state(page).is_swapped() {
            uprintln!(
                uart,
                "Swapping page {}: {:?}",
                page,
                state.get_page_state(page)
            );
            match state.get_page_state(page) {
                PageState::Original => {
                    // We need to copy the A page to a scratch page

                    // Decide which scratch page to use
                    let scratch_page = bootloader_scratch_page_range().start + scratch_page_index;
                    let scratch_address = scratch_page * PAGE_SIZE;

                    uprintln!(
                        uart,
                        "Moving page @{:#010X} to page {:#010X}",
                        slot_a_address,
                        scratch_address
                    );

                    // Erase the scratch area
                    flash.erase_page(scratch_address);
                    // Program the data from slot A into the scratch slot
                    flash.program_page(scratch_address, unsafe {
                        core::slice::from_raw_parts(
                            slot_a_address as *const u32,
                            PAGE_SIZE as usize / core::mem::size_of::<u32>(),
                        )
                    });
                    // Update the state
                    state.set_page_state(page, PageState::InScratch { scratch_page });
                    state.burn_store(flash);
                }
                PageState::InScratch { scratch_page } => {
                    // We need to copy the B page to the A slot

                    uprintln!(
                        uart,
                        "Moving page @{:#010X} to page {:#010X}",
                        slot_b_address,
                        slot_a_address
                    );

                    // Erase the A page
                    flash.erase_page(slot_a_address);
                    // Program the data from slot B into the A slot
                    flash.program_page(slot_a_address, unsafe {
                        core::slice::from_raw_parts(
                            slot_b_address as *const u32,
                            PAGE_SIZE as usize / core::mem::size_of::<u32>(),
                        )
                    });
                    // Update the state
                    state.set_page_state(page, PageState::InScratchOverwritten { scratch_page });
                    state.burn_store(flash);
                }
                PageState::InScratchOverwritten { scratch_page } => {
                    // We need to copy the scratch page to the B slot

                    let scratch_address = scratch_page * PAGE_SIZE;

                    uprintln!(
                        uart,
                        "Moving page @{:#010X} to page {:#010X}",
                        scratch_address,
                        slot_b_address
                    );

                    // Erase the B page
                    flash.erase_page(slot_b_address);
                    // Program the data from the scratch slot into the B slot
                    flash.program_page(slot_b_address, unsafe {
                        core::slice::from_raw_parts(
                            scratch_address as *const u32,
                            PAGE_SIZE as usize / core::mem::size_of::<u32>(),
                        )
                    });
                    // Update the state
                    state.set_page_state(page, PageState::Swapped);

                    state.burn_store(flash);
                }
                PageState::Swapped => {
                    // We're done and shouldn't be able to get here
                    unreachable!()
                }
            }
        }

        scratch_page_index = (scratch_page_index + 1) % total_scratch_pages;
    }

    // We're done, so we should change the state
    if test_swap {
        state.set_goal(BootloaderGoal::StartSwap);
    } else {
        state.set_goal(BootloaderGoal::JumpToApplication);
    }

    state.store(flash);
}

async fn jump_to_application(mut uart: Uart) -> ! {
    // The application may not be stationed at the start of its slot.
    // We need to search for it first.
    // We will jump to the first non-erased (0xFFFF_FFFF) word if that could be a pointer to a reset vector inside the program_slot_a_range
    let mut application_address = None;

    for possible_address in program_slot_a_range().step_by(4) {
        // We can read this address safely because it will always be in flash
        let address_value = unsafe { (possible_address as *const u32).read_volatile() };

        match address_value {
            0xFFFF_FFFF => continue,
            _ if program_slot_a_range().contains(&address_value) => {
                application_address = Some(possible_address);
                break;
            }
            _ => break,
        }
    }

    match application_address {
        Some(application_address) => {
            uprintln!(uart, "Jumping to {:#08X}", application_address);

            // We need to disable all used peripherals
            drop(uart);

            unsafe { cortex_m::asm::bootload(application_address as *const u32) }
        }
        None => panic!("Could not find a reset vector in the firmware"),
    }
}

#[cortex_m_rt::exception]
unsafe fn HardFault(frame: &cortex_m_rt::ExceptionFrame) -> ! {
    panic!("{:?}", frame);
}
