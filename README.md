# nRF9160 Bootloader

This is a 100% Rust bootloader for the nRF9160 microcontroller.

Currently, building this project requires a recent nightly compile because is relies on Embassy for the serial output.

## Structure

The project is split in two:

- shared: Exposes all types that both the bootloader and application needs to be able to access.
- bootloader: The binary part of the project

## Workings

The bootloader has four special memory regions which are defined in the `memory.x` file.
There are two firmware slots, A & B, as well as a bootloader state area and some scratch space.

The bootloader will jump to the application in firmware slot A.

The application can fill slot B with an updated firmware. Then it needs to set the bootloader goal to either `StartSwap` or `StartTestSwap` and reboot.
When the bootloader sees that it should swap the two firmware slots it will do that in a way so that any cut in power or reset will not lead to a currupt device.

```txt
+--------+          2.         +--------+
| Slot A |<--------------------+ Slot B |
|        |                     |        |
|        +-------+     +------>|        |
+--------+       |     |       +--------+
|        |    1. |     | 3.    |        |
|        |       |     |       |        |
|        |       |     |       |        |
+--------+       |     |       +--------+
|        |       |     |       |        |
|        |       |     |       |        |
|        |       |     |       |        |
+--------+       |     |       +--------+
|        |       |     |       |        |
|        |       |     |       |        |
|        |       |     |       |        |
+--------+       |     |       +--------+
|        |       |     |       |        |
|        |       |     |       |        |
|        |       |     |       |        |
+--------+       |     |       +--------+
|        |       |     |       |        |
|        |       |     |       |        |
|        |       |     |       |        |
+--------+       |     |       +--------+
                 v     |
               +-------+-+
               | Scratch |
               |         |
               +---------+
               |         |
               |         |
               +---------+
               |         |
               |         |
               +---------+
```

First a page of slot A is written to a scratch page. There are multiple scratch pages because flash will wear out when erased.
The second step is to move the B page to the A slot. The third and final step is to move the page in scratch to the B slot.

The state of each page is written in the bootloader state without doing an erase. At every step of the way we know where each page is so that we can resume the swap at any point.

When the bootloader is done with everything it needs to jump to the application.

The address of the application is unknown still so it needs to be searched for.
The bootloader will go through the memory of the firmware A slot word by word in search of the vector table.

If a word is 0xFFFF_FFFF or 0x0000_0000 then it is ignored because the assumption is that it is some kind of padding.
The first word that is not ignored should be the initial stack pointer and the value is checked to see if it is located somewhere in RAM. If it is not, then the bootloader will panic and restart.
The second word (the one after the initial stack pointer) should be the reset vector. It is checked that the reset vector lies somewhere in slot A. If it is not, the bootloader will panic and reboot.

So as long as the application has 'clean' padding, the application can be put anywhere in its slot.
After the vector table, the image may have arbitrary data. There is no image header or trailer.

With the knowledge that the initial stack pointer and reset vector are ther, we can be quite sure that we've found a vector table.

All peripherals are reset and then the bootloader performs the bootload operation as part of the `cortex-m` crate.
