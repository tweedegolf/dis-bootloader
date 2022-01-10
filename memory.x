MEMORY
{
    FLASH                    : ORIGIN = 0x00000000, LENGTH = 64K

    PROGRAM_SLOT_A_FLASH     : ORIGIN = 0x00010000, LENGTH = 448K
    PROGRAM_SLOT_B_FLASH     : ORIGIN = 0x00080000, LENGTH = 448K

    # Application data       : ORIGIN = 0x000F0000, LENGTH = 32K

    BOOTLOADER_SCRATCH_FLASH : ORIGIN = 0x000F8000, LENGTH = 28K
    BOOTLOADER_STATE_FLASH   : ORIGIN = 0x000FF000, LENGTH = 4K

    RAM   : ORIGIN = 0x20000000, LENGTH = 64K
}

_bootloader_flash_start = ORIGIN(FLASH);
_bootloader_flash_end = _bootloader_flash_start + LENGTH(FLASH);
_bootloader_scratch_start = ORIGIN(BOOTLOADER_SCRATCH_FLASH);
_bootloader_scratch_end = _bootloader_scratch_start + LENGTH(BOOTLOADER_SCRATCH_FLASH);
_bootloader_state_start = ORIGIN(BOOTLOADER_STATE_FLASH);
_bootloader_state_end = _bootloader_state_start + LENGTH(BOOTLOADER_STATE_FLASH);

_program_slot_a_start = ORIGIN(PROGRAM_SLOT_A_FLASH);
_program_slot_a_end = _program_slot_a_start + LENGTH(PROGRAM_SLOT_A_FLASH);
_program_slot_b_start = ORIGIN(PROGRAM_SLOT_B_FLASH);
_program_slot_b_end = _program_slot_b_start + LENGTH(PROGRAM_SLOT_B_FLASH);

ASSERT(_bootloader_scratch_start % 0x1000 == 0, "Flash area must align with flash pages");
ASSERT(_bootloader_state_start % 0x1000 == 0, "Flash area must align with flash pages");
ASSERT((_bootloader_state_end - _bootloader_state_start) == 4096, "Bootloader state area must have a size of 4K");
ASSERT(_program_slot_a_start % 0x1000 == 0, "Flash area must align with flash pages");
ASSERT(_program_slot_b_start % 0x1000 == 0, "Flash area must align with flash pages");
