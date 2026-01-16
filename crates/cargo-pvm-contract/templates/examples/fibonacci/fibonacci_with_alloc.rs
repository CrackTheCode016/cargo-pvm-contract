#![no_main]
#![no_std]

use alloy_core::{sol, sol_types::SolCall};
use pallet_revive_uapi::{HostFn, HostFnImpl as api, ReturnFlags};

extern crate alloc;
use alloc::vec;

sol!("Fibonacci.sol");

#[global_allocator]
static mut ALLOC: picoalloc::Mutex<picoalloc::Allocator<picoalloc::ArrayPointer<1024>>> = {
    static mut ARRAY: picoalloc::Array<1024> = picoalloc::Array([0u8; 1024]);

    picoalloc::Mutex::new(picoalloc::Allocator::new(unsafe {
        picoalloc::ArrayPointer::new(&raw mut ARRAY)
    }))
};

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    // Safety: The unimp instruction is guaranteed to trap
    unsafe {
        core::arch::asm!("unimp");
        core::hint::unreachable_unchecked();
    }
}

/// This is the constructor which is called once per contract.
#[polkavm_derive::polkavm_export]
pub extern "C" fn deploy() {}

/// This is the regular entry point when the contract is called.
#[polkavm_derive::polkavm_export]
pub extern "C" fn call() {
    let call_data_len = api::call_data_size();
    let mut call_data = vec![0u8; call_data_len as usize];
    api::call_data_copy(&mut call_data, 0);

    let selector: [u8; 4] = call_data[0..4].try_into().unwrap();

    match selector {
        Fibonacci::fibonacciCall::SELECTOR => {
            let fibonacci_call = Fibonacci::fibonacciCall::abi_decode(&call_data, true)
                .expect("Failed to decode fibonacci call");

            let result = _fibonacci(fibonacci_call._0);
            let returns = Fibonacci::fibonacciCall::abi_encode_returns(&(result,));
            api::return_value(ReturnFlags::empty(), &returns);
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
