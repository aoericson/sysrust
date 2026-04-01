#![no_std]
#![no_main]

extern crate alloc;
extern crate sysrust_rt;

use alloc::vec::Vec;
use alloc::string::String;
use sysrust_rt::sys;

#[unsafe(no_mangle)]
fn main() -> i32 {
    sys::println("=== sysrust alloc test ===");

    // Test Vec
    let mut v: Vec<i32> = Vec::new();
    for i in 0..10 {
        v.push(i * i);
    }
    let sum: i32 = v.iter().sum();
    if sum != 285 {
        sys::println("FAIL: Vec sum incorrect");
        return 1;
    }
    sys::println("Vec: OK (0..10 squares sum = 285)");

    // Test String
    let mut s = String::from("Hello");
    s.push_str(" from sysrust!");
    sys::print("String: ");
    sys::write(1, s.as_bytes());
    sys::write(1, b"\n");

    // Test larger allocation
    let big: Vec<u8> = alloc::vec![0xAA; 4096];
    if big.len() == 4096 && big[0] == 0xAA && big[4095] == 0xAA {
        sys::println("Large alloc: OK (4KB)");
    } else {
        sys::println("FAIL: Large alloc");
        return 1;
    }

    sys::println("=== ALL PASSED ===");
    0
}
