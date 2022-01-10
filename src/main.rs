#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]

use crate::flash_addresses::*;
use rtt_target::{rprintln, rtt_init_print};
use state::BootloaderState;

mod flash_addresses;
mod state;

#[cortex_m_rt::entry]
fn main() -> ! {
    let _cp = cortex_m::Peripherals::take().unwrap();
    let _dp = nrf9160_pac::Peripherals::take().unwrap();

    rtt_init_print!();

    rprintln!("Starting bootloader");
    rprintln!("Defined memory regions:");
    rprintln!("\tbootloader flash:   {:010X?}", bootloader_flash_range());
    rprintln!("\tbootloader scratch: {:010X?}", bootloader_scratch_range());
    rprintln!("\tbootloader state:   {:010X?}", bootloader_state_range());
    rprintln!("\tprogram slot a:     {:010X?}", program_slot_a_range());
    rprintln!("\tprogram slot b:     {:010X?}", program_slot_b_range());

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
        state::BootloaderGoal::StartSwap => todo!(),
        state::BootloaderGoal::FinishSwap => todo!(),
        state::BootloaderGoal::StartTestSwap => todo!(),
        state::BootloaderGoal::FinishTestSwap => todo!(),
    }

    cortex_m::asm::bkpt();
    loop {}
}

fn jump_to_application() -> ! {
    todo!()
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
