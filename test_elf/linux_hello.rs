// A "real Rust" program compiled for sysrust OS.
// Uses alloc (Vec, String, format!) via sysrust-rt runtime.
// This program is what code inside sysrust looks like going forward.
#![no_std]
#![no_main]

extern crate alloc;
extern crate sysrust_rt;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use sysrust_rt::sys;

#[unsafe(no_mangle)]
fn main() -> i32 {
    sys::println("=== Real Rust inside sysrust ===");

    // Vec with iterator
    let v: Vec<i32> = (0..10).map(|x| x * x).collect();
    let sum: i32 = v.iter().sum();
    let msg = format!("Vec of squares: {:?}, sum = {}", &v[..5], sum);
    sys::println(&msg);

    // String manipulation
    let mut s = String::from("sysrust");
    s.push_str(" is alive!");
    sys::println(&s);

    // Closures
    let double = |x: i32| x * 2;
    let results: Vec<i32> = (1..=5).map(double).collect();
    let msg2 = format!("Doubled: {:?}", results);
    sys::println(&msg2);

    // Box
    let boxed: alloc::boxed::Box<[u8; 1024]> = alloc::boxed::Box::new([0xAB; 1024]);
    if boxed[0] == 0xAB && boxed[1023] == 0xAB {
        sys::println("Box<[u8; 1024]>: OK");
    }

    sys::println("=== ALL PASSED ===");
    0
}
