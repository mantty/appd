// Compiled with:
//   rustc --edition 2024 --target wasm32-unknown-unknown -O --crate-type=cdylib \
//     -o word_picker.wasm word_picker.rs
#![no_std]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn pick_index(seed: u32, count: u32) -> u32 {
    seed % count
}
