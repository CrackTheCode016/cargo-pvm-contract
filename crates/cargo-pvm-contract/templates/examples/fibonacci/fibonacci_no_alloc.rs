#![no_main]
#![no_std]

use pallet_revive_uapi::{HostFn, HostFnImpl as api, ReturnFlags};

// ============================================================================
// FIBONACCI CONTRACT - Generated from Solidity ABI
// ============================================================================

// Function selectors

const FIBONACCI_SELECTOR: [u8; 4] = [0xe4, 0x44, 0xa7, 0x09]; // fibonacci(uint32)

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    // Safety: The unimp instruction is guaranteed to trap
    unsafe {
        core::arch::asm!("unimp");
        core::hint::unreachable_unchecked();
    }
}

/// Contract entry points.

/// This is the constructor which is called once per contract.
#[polkavm_derive::polkavm_export]
pub extern "C" fn deploy() {}

/// This is the regular entry point when the contract is called.
#[polkavm_derive::polkavm_export]
pub extern "C" fn call() {
    let call_data_len = api::call_data_size() as usize;

    // Fixed buffer for call data
    let mut call_data = [0u8; 256];
    if call_data_len > call_data.len() {
        panic!("Call data too large");
    }

    api::call_data_copy(&mut call_data[..call_data_len], 0);

    if call_data_len < 4 {
        panic!("Call data too short");
    }

    let selector: [u8; 4] = call_data[0..4].try_into().unwrap();

    match selector {
        FIBONACCI_SELECTOR => {
            if call_data_len < 36 {
                panic!("Invalid fibonacci call data");
            }

            let mut input = [0u8; 4];
            api::call_data_copy(&mut input, 32);

            let n = u32::from_be_bytes(input);
            let result = _fibonacci(n);
            let output = result.to_be_bytes();

            let mut response = [0u8; 32];
            response[28..].copy_from_slice(&output);
            api::return_value(ReturnFlags::empty(), &response);
        }

        _ => panic!("Unknown function selector"),
    }
}

fn _fibonacci(n: u32) -> u32 {
    if n == 0 {
        0
    } else if n == 1 {
        1
    } else {
        _fibonacci(n - 1) + _fibonacci(n - 2)
    }
}
