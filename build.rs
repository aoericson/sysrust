// build.rs -- Compile assembly files and link them with the Rust kernel.

use std::process::Command;
use std::path::Path;

fn main() {
    let asm_files = ["boot/boot.s", "boot/isr.s"];

    for asm in &asm_files {
        let src = Path::new(asm);
        let obj = Path::new("target")
            .join(src.file_stem().unwrap())
            .with_extension("o");

        // Ensure output directory exists
        if let Some(parent) = obj.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }

        let status = Command::new("nasm")
            .args(["-f", "elf64", &src.to_string_lossy(), "-o", &obj.to_string_lossy()])
            .status()
            .expect("failed to run nasm");

        if !status.success() {
            panic!("nasm failed for {}", asm);
        }

        println!("cargo:rustc-link-arg={}", obj.display());
        println!("cargo:rerun-if-changed={}", asm);
    }

    println!("cargo:rerun-if-changed=linker.ld");
}
