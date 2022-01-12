#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]

use dis_bootloader::{
    flash::{erase_page, program_page},
    flash_addresses::{
        bootloader_flash_page_range, bootloader_flash_range, bootloader_scratch_page_range,
        bootloader_scratch_range, bootloader_state_page_range, bootloader_state_range,
        program_slot_a_page_range, program_slot_a_range, program_slot_b_page_range,
        program_slot_b_range, PAGE_SIZE,
    },
    reset_reason::ResetReason,
    state::{BootloaderGoal, BootloaderState, PageState},
};
use embassy_nrf::{
    gpio::NoPin,
    interrupt,
    peripherals::UARTETWISPI0,
    uarte::{self, Uarte},
};
use embassy_traits::uart::Write;
use panic_persist::get_panic_message_bytes;

type Uart = Uarte<'static, UARTETWISPI0>;

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
    let nvmc = unsafe { &*embassy_nrf::pac::NVMC::PTR };

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

    let reset_reason = ResetReason::lookup(unsafe { &*embassy_nrf::pac::POWER::PTR });
    ResetReason::clear(unsafe { &*embassy_nrf::pac::POWER::PTR });

    if reset_reason == ResetReason::Lockup {
        cortex_m::asm::delay(u32::MAX);
    }

    uprintln!(uart, "Starting bootloader. Reset reason: {}", reset_reason);

    // Check if there was a panic message, if so, send to UART
    if let Some(msg) = get_panic_message_bytes() {
        uprintln!(uart, "Booted up from a panic:");
        uart.write(msg).await.unwrap();
    }

    uprintln!(uart, "Defined memory regions:");
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
        jump_to_application(&mut uart).await;
    }

    let goal = state.goal();
    uprintln!(uart, "Goal: {:?}", goal);

    match goal {
        BootloaderGoal::JumpToApplication => {
            jump_to_application(&mut uart).await;
        }
        BootloaderGoal::StartSwap => {
            state.prepare_swap(false, nvmc);
            perform_swap(false, &mut state, nvmc, &mut uart).await;
            jump_to_application(&mut uart).await;
        }
        BootloaderGoal::FinishSwap => {
            perform_swap(false, &mut state, nvmc, &mut uart).await;
            jump_to_application(&mut uart).await;
        }
        BootloaderGoal::StartTestSwap => {
            state.prepare_swap(true, nvmc);
            perform_swap(true, &mut state, nvmc, &mut uart).await;
            jump_to_application(&mut uart).await;
        }
        BootloaderGoal::FinishTestSwap => {
            perform_swap(true, &mut state, nvmc, &mut uart).await;
            jump_to_application(&mut uart).await;
        }
    }

    loop {}
}

async fn perform_swap(
    test_swap: bool,
    state: &mut BootloaderState,
    flash: &embassy_nrf::pac::nvmc::RegisterBlock,
    uart: &mut Uart,
) {
    let total_program_pages = program_slot_a_page_range().len() as u32;
    let total_scratch_pages = bootloader_scratch_page_range().len() as u32;

    uprintln!(uart, "total_program_pages: {}", total_program_pages,);
    uprintln!(uart, "total_scratch_pages: {}", total_scratch_pages,);

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
                    erase_page(scratch_address, flash);
                    // Program the data from slot A into the scratch slot
                    program_page(
                        scratch_address,
                        unsafe {
                            core::slice::from_raw_parts(
                                slot_a_address as *const u32,
                                PAGE_SIZE as usize / core::mem::size_of::<u32>(),
                            )
                        },
                        flash,
                    );
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
                    erase_page(slot_a_address, flash);
                    // Program the data from slot B into the A slot
                    program_page(
                        slot_a_address,
                        unsafe {
                            core::slice::from_raw_parts(
                                slot_b_address as *const u32,
                                PAGE_SIZE as usize / core::mem::size_of::<u32>(),
                            )
                        },
                        flash,
                    );
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
                    erase_page(slot_b_address, flash);
                    // Program the data from the scratch slot into the B slot
                    program_page(
                        slot_b_address,
                        unsafe {
                            core::slice::from_raw_parts(
                                scratch_address as *const u32,
                                PAGE_SIZE as usize / core::mem::size_of::<u32>(),
                            )
                        },
                        flash,
                    );
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

async fn jump_to_application(uart: &mut Uart) -> ! {
    let application_address = program_slot_a_range().start + 0x200; // We use a fixed offset here because because the SPM binary still has the MCUboot header. Very ugly

    uprintln!(uart, "Jumping to {:#08X}", application_address);

    unsafe { cortex_m::asm::bootload(application_address as *const u32) }
}

#[cortex_m_rt::exception]
unsafe fn HardFault(frame: &cortex_m_rt::ExceptionFrame) -> ! {
    panic!("{:?}", frame);
}
