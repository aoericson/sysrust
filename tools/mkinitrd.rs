use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;

const NAME_LEN: usize = 64;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: {} <output> <file1> [file2] ...", args[0]);
        std::process::exit(1);
    }
    let mut out = fs::File::create(&args[1]).expect("cannot create output file");
    let count = (args.len() - 2) as u32;
    out.write_all(&count.to_le_bytes()).unwrap();
    for i in 2..args.len() {
        let path = Path::new(&args[i]);
        let basename = path.file_name().unwrap().to_str().unwrap();
        let mut name = [0u8; NAME_LEN];
        let bytes = basename.as_bytes();
        let n = bytes.len().min(NAME_LEN - 1);
        name[..n].copy_from_slice(&bytes[..n]);
        let data = fs::read(&args[i]).expect("cannot read file");
        let size = data.len() as u32;
        out.write_all(&name).unwrap();
        out.write_all(&size.to_le_bytes()).unwrap();
        out.write_all(&data).unwrap();
    }
    println!("Created initrd with {} files", count);
}
