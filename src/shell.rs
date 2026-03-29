// shell.rs -- Interactive command-line shell.
//
// A simple read-eval-print loop. Reads a line of input from the keyboard
// or serial port (whichever has data first), matches it against built-in
// commands, and executes the matching command.
//
// Built-in commands:
//   help, clear, echo, ls, cat, mem, ping, resolve, touch, write, edit,
//   rc, run, tcp, http, save, load, threads, spawn, mirror, reboot

use crate::vga::{self, Color};
use crate::keyboard;
use crate::serial;
use crate::string;
use crate::timer;
use crate::pmm;
use crate::heap;
use crate::vfs::{self, VfsNode, VFS_ROOT, VFS_DIRECTORY};
use crate::icmp;
use crate::dns;
use crate::net;
use crate::tcp;
use crate::fat16;
use crate::rc;
use crate::elf;
use crate::syscall;
use crate::recovery;
use crate::thread;
use crate::editor;

use core::arch::asm;

const INPUT_SIZE: usize = 256;

static mut INPUT: [u8; INPUT_SIZE] = [0; INPUT_SIZE];
static mut INPUT_LEN: usize = 0;

// ---------------------------------------------------------------------------
// Console I/O layer -- reads from keyboard OR serial (whichever has data
// first), writes to VGA (serial mirroring handled by VGA when enabled).
// ---------------------------------------------------------------------------

/// Block until a character is available from serial or keyboard.
unsafe fn console_getchar() -> u8 {
    loop {
        if serial::data_ready() {
            return serial::getchar();
        }
        if keyboard::data_ready() {
            return keyboard::getchar();
        }
        asm!("hlt");
    }
}

/// Write a single character to VGA (serial mirroring handled by VGA layer).
unsafe fn console_putchar(c: u8) {
    vga::putchar(c);
}

/// Write a null-terminated byte string to VGA.
unsafe fn console_puts(s: &[u8]) {
    vga::puts(s);
}

/// Print the shell prompt in green.
unsafe fn shell_prompt() {
    vga::set_color(Color::LightGreen, Color::Black);
    console_puts(b"> ");
    vga::set_color(Color::LightGrey, Color::Black);
}

/// Read one line of input from keyboard or serial.
/// Characters are echoed. Backspace removes the last character.
/// Enter terminates the line.
unsafe fn shell_readline() {
    INPUT_LEN = 0;

    loop {
        let c = console_getchar();

        if c == b'\n' || c == b'\r' {
            console_putchar(b'\n');
            INPUT[INPUT_LEN] = 0;
            return;
        }

        // Backspace (0x08) or DEL (127, common serial backspace)
        if c == b'\x08' || c == 127 {
            if INPUT_LEN > 0 {
                INPUT_LEN -= 1;
                console_putchar(b'\x08');
            }
            continue;
        }

        // Append printable characters (up to buffer limit)
        if INPUT_LEN < INPUT_SIZE - 1 {
            INPUT[INPUT_LEN] = c;
            INPUT_LEN += 1;
            console_putchar(c);
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Print an unsigned integer to VGA.
unsafe fn print_uint(val: u32) {
    if val == 0 {
        vga::putchar(b'0');
        return;
    }
    let mut buf = [0u8; 12];
    let mut i = 0usize;
    let mut v = val;
    while v > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        vga::putchar(buf[i]);
    }
}

/// Parse "A.B.C.D" into a u32 IP address (host byte order).
/// Returns 0 on parse error.
unsafe fn parse_ip(s: *const u8) -> u32 {
    let mut ip: u32 = 0;
    let mut p = s;

    for i in 0..4u32 {
        let mut octet: u32 = 0;
        if *p < b'0' || *p > b'9' {
            return 0;
        }
        while *p >= b'0' && *p <= b'9' {
            octet = octet * 10 + (*p - b'0') as u32;
            p = p.add(1);
        }
        if octet > 255 {
            return 0;
        }
        ip = (ip << 8) | octet;
        if i < 3 {
            if *p != b'.' {
                return 0;
            }
            p = p.add(1);
        }
    }
    ip
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

unsafe fn cmd_help() {
    vga::puts(b"Available commands:\n");
    vga::puts(b"  help    - show this message\n");
    vga::puts(b"  clear   - clear the screen\n");
    vga::puts(b"  echo    - echo text (usage: echo <text>)\n");
    vga::puts(b"  ls      - list files and directories\n");
    vga::puts(b"  cat     - print file contents (usage: cat <path>)\n");
    vga::puts(b"  mem     - show memory usage\n");
    vga::puts(b"  ping    - ping an IP or hostname (usage: ping <ip|host>)\n");
    vga::puts(b"  resolve - resolve a hostname (usage: resolve <hostname>)\n");
    vga::puts(b"  touch   - create an empty file (usage: touch <name>)\n");
    vga::puts(b"  write   - write text to a file (usage: write <name> <text>)\n");
    vga::puts(b"  edit    - text editor (usage: edit <filename>)\n");
    vga::puts(b"  rc      - compile a Rust source file (usage: rc <file.rs>)\n");
    vga::puts(b"  run     - run compiled program (usage: run <binary>)\n");
    vga::puts(b"  tcp     - TCP client (usage: tcp <ip|host> <port>)\n");
    vga::puts(b"  http    - HTTP GET (usage: http <ip|host> [path])\n");
    vga::puts(b"  save    - save file to disk (usage: save <filename>)\n");
    vga::puts(b"  load    - load file from disk (usage: load <filename>)\n");
    vga::puts(b"  threads - show active kernel threads\n");
    vga::puts(b"  spawn   - spawn a background test thread\n");
    vga::puts(b"  mirror  - toggle serial mirroring of VGA output\n");
    vga::puts(b"  reboot  - reboot the system\n");
}

unsafe fn cmd_echo() {
    if INPUT_LEN > 5 {
        vga::puts(&INPUT[5..INPUT_LEN]);
        vga::putchar(b'\n');
    } else {
        vga::putchar(b'\n');
    }
}

/// Reboot by triggering a triple fault.
/// Load an IDT with limit=0 (no valid entries), then fire interrupt 3.
unsafe fn cmd_reboot() {
    #[repr(C, packed)]
    struct NullIdt {
        limit: u16,
        base: u32,
    }
    let null_idt = NullIdt { limit: 0, base: 0 };
    asm!("lidt [{}]", in(reg) &null_idt as *const NullIdt, options(nostack));
    asm!("int 0x03", options(nostack));
}

unsafe fn cmd_mem() {
    let total = pmm::get_total_pages();
    let free_pages = pmm::get_free_pages();
    let used = total - free_pages;
    let heap_free = heap::get_free();

    vga::puts(b"Physical memory:\n");

    vga::puts(b"  Total: ");
    print_uint(total);
    vga::puts(b" pages (");
    print_uint(total * 4 / 1024);
    vga::puts(b" MB)\n");

    vga::puts(b"  Free:  ");
    print_uint(free_pages);
    vga::puts(b" pages (");
    print_uint(free_pages * 4 / 1024);
    vga::puts(b" MB)\n");

    vga::puts(b"  Used:  ");
    print_uint(used);
    vga::puts(b" pages (");
    print_uint(used * 4 / 1024);
    vga::puts(b" MB)\n");

    vga::puts(b"Heap (0x00500000):\n");
    vga::puts(b"  Free:  ");
    print_uint(heap_free / 1024);
    vga::puts(b" KB\n");
}

unsafe fn cmd_ping() {
    if INPUT_LEN <= 5 {
        vga::puts(b"Usage: ping <ip>\n");
        return;
    }

    // Null-terminate the argument
    let arg = INPUT[5..INPUT_LEN].as_ptr();
    INPUT[INPUT_LEN] = 0;

    let mut target_ip = parse_ip(arg);
    if target_ip == 0 {
        // Not a valid IP -- try DNS resolution
        if dns::resolve(arg, &mut target_ip) == 0 {
            vga::puts(b"Could not resolve host\n");
            return;
        }
    }

    let mut ip_str = [0u8; 16];
    net::ip_to_str(target_ip, ip_str.as_mut_ptr());
    vga::puts(b"Pinging ");
    vga::puts(&ip_str);
    vga::puts(b"...\n");

    for seq in 1..=4u16 {
        let start_tick = timer::get_ticks();
        let mut r_id: u16 = 0;
        let mut r_seq: u16 = 0;
        let mut r_ip: u32 = 0;
        let mut got_reply = false;

        icmp::send_echo_request(target_ip, 1, seq);

        // Use elapsed comparison to avoid overflow
        while timer::get_ticks().wrapping_sub(start_tick) < 200 {
            if icmp::got_reply(&mut r_id, &mut r_seq, &mut r_ip) != 0 {
                let elapsed = timer::get_ticks().wrapping_sub(start_tick) * 10;
                vga::puts(b"Reply from ");
                net::ip_to_str(r_ip, ip_str.as_mut_ptr());
                vga::puts(&ip_str);
                vga::puts(b": seq=");
                print_uint(r_seq as u32);
                vga::puts(b" time=");
                print_uint(elapsed);
                vga::puts(b"ms\n");
                got_reply = true;
                break;
            }
            asm!("hlt");
        }
        if !got_reply {
            vga::puts(b"Request timed out.\n");
        }

        // Wait 100ms between pings
        if seq < 4 {
            timer::wait(10);
        }
    }
}

unsafe fn cmd_resolve() {
    if INPUT_LEN <= 8 {
        vga::puts(b"Usage: resolve <hostname>\n");
        return;
    }

    INPUT[INPUT_LEN] = 0;
    let arg = INPUT[8..INPUT_LEN].as_ptr();

    vga::puts(b"Resolving ");
    vga::puts(&INPUT[8..INPUT_LEN]);
    vga::puts(b"...\n");

    let mut ip: u32 = 0;
    if dns::resolve(arg, &mut ip) != 0 {
        let mut ip_str = [0u8; 16];
        net::ip_to_str(ip, ip_str.as_mut_ptr());
        vga::puts(&ip_str);
        vga::putchar(b'\n');
    } else {
        vga::puts(b"DNS resolution failed\n");
    }
}

unsafe fn cmd_ls() {
    let dir: *mut VfsNode;

    // Support "ls <path>" -- default to root if no argument
    if INPUT_LEN > 3 && INPUT[2] == b' ' {
        INPUT[INPUT_LEN] = 0;
        let path = INPUT[3..INPUT_LEN].as_ptr();

        let mut found_dir = vfs::finddir(VFS_ROOT, path);

        // Try stripping leading slash for paths like "/disk"
        if found_dir.is_null() && *path == b'/' {
            let mut p = path.add(1);
            let mut current = VFS_ROOT;
            while *p != 0 && !current.is_null() {
                let mut component = [0u8; 64];
                let mut ci = 0usize;
                while *p != 0 && *p != b'/' && ci < 63 {
                    component[ci] = *p;
                    ci += 1;
                    p = p.add(1);
                }
                component[ci] = 0;
                while *p == b'/' {
                    p = p.add(1);
                }
                current = vfs::finddir(current, component.as_ptr());
            }
            found_dir = current;
        }

        if found_dir.is_null() || (*found_dir).flags & VFS_DIRECTORY == 0 {
            vga::puts(b"Not a directory: ");
            vga::puts(&INPUT[3..INPUT_LEN]);
            vga::putchar(b'\n');
            return;
        }
        dir = found_dir;
    } else {
        dir = VFS_ROOT;
    }

    let mut i: u32 = 0;
    let mut found = false;
    loop {
        let node = vfs::readdir(dir, i);
        if node.is_null() {
            break;
        }
        found = true;
        if (*node).flags & VFS_DIRECTORY != 0 {
            vga::set_color(Color::LightCyan, Color::Black);
            vga::puts(&(*node).name);
            vga::puts(b"/\n");
            vga::set_color(Color::LightGrey, Color::Black);
        } else {
            vga::puts(&(*node).name);
            vga::puts(b"  (");
            print_uint((*node).size);
            vga::puts(b" bytes)\n");
        }
        i += 1;
    }
    if !found {
        vga::puts(b"(no files)\n");
    }
}

unsafe fn cmd_cat() {
    if INPUT_LEN <= 4 {
        vga::puts(b"Usage: cat <filename>\n");
        return;
    }

    INPUT[INPUT_LEN] = 0;
    let path = INPUT[4..INPUT_LEN].as_ptr();

    let fd = vfs::vfs_open(path);
    if fd < 0 {
        vga::puts(b"File not found: ");
        vga::puts(&INPUT[4..INPUT_LEN]);
        vga::putchar(b'\n');
        return;
    }

    // Safety limit for device files like /dev/zero (4 KB max output)
    let mut total: u32 = 0;
    let mut buf = [0u8; 256];
    loop {
        let n = vfs::vfs_fd_read(fd, buf.as_mut_ptr(), buf.len() as u32);
        if n <= 0 {
            break;
        }
        for j in 0..n as usize {
            vga::putchar(buf[j]);
        }
        total += n as u32;
        if total >= 4096 {
            vga::puts(b"\n[output truncated]\n");
            break;
        }
    }

    vfs::vfs_close(fd);
}

unsafe fn cmd_threads() {
    static STATE_NAMES: [&[u8]; 4] = [
        b"READY\0",
        b"RUNNING\0",
        b"BLOCKED\0",
        b"DEAD\0",
    ];

    vga::puts(b"ID  State\n");
    vga::puts(b"--  -------\n");

    for i in 0..thread::MAX_THREADS {
        let t = thread::get(i as i32);
        if t.is_null() {
            continue;
        }
        if (*t).state == thread::ThreadState::Dead {
            continue;
        }
        print_uint((*t).id);
        vga::puts(b"   ");
        let state = (*t).state as usize;
        if state < 4 {
            vga::puts(STATE_NAMES[state]);
        } else {
            vga::puts(b"UNKNOWN");
        }
        vga::putchar(b'\n');
    }

    vga::puts(b"Active threads: ");
    print_uint(thread::get_count() as u32);
    vga::putchar(b'\n');
}

/// Background test thread: prints a message every ~500ms then exits
/// after 5 iterations. Demonstrates preemptive multitasking.
extern "C" fn test_thread_func_c() {
    unsafe {
        let tid = thread::get_current();

        for i in 0..5u32 {
            vga::puts(b"[thread ");
            print_uint(tid as u32);
            vga::puts(b"] tick ");
            print_uint(i);
            vga::putchar(b'\n');
            timer::wait(50); // ~500ms
        }
        vga::puts(b"[thread ");
        print_uint(tid as u32);
        vga::puts(b"] done\n");
    }
}

unsafe fn cmd_spawn() {
    let tid = thread::create(test_thread_func_c);
    if tid < 0 {
        vga::puts(b"Failed to create thread (table full)\n");
        return;
    }
    vga::puts(b"Spawned thread ");
    print_uint(tid as u32);
    vga::putchar(b'\n');
}

unsafe fn cmd_touch() {
    if INPUT_LEN <= 6 {
        vga::puts(b"Usage: touch <filename>\n");
        return;
    }

    INPUT[INPUT_LEN] = 0;
    let name = INPUT[6..INPUT_LEN].as_ptr();

    let node = vfs::create_file(name);
    if !node.is_null() {
        vga::puts(b"Created: ");
        vga::puts(&INPUT[6..INPUT_LEN]);
        vga::putchar(b'\n');
    } else {
        vga::puts(b"Failed to create file\n");
    }
}

unsafe fn cmd_write() {
    if INPUT_LEN <= 6 {
        vga::puts(b"Usage: write <filename> <text>\n");
        return;
    }

    INPUT[INPUT_LEN] = 0;

    // Find the space between filename and text
    let filename_start = 6;
    let filename_ptr = INPUT[filename_start..].as_ptr();
    let space = string::strchr(filename_ptr, b' ');
    if space.is_null() {
        vga::puts(b"Usage: write <filename> <text>\n");
        return;
    }

    // Null-terminate the filename in-place
    let space_offset = (space as usize) - (INPUT.as_ptr() as usize);
    INPUT[space_offset] = 0;
    let text_ptr = INPUT[space_offset + 1..].as_ptr();

    let mut node = vfs::finddir(VFS_ROOT, filename_ptr);
    if node.is_null() {
        node = vfs::create_file(filename_ptr);
    }
    if node.is_null() {
        vga::puts(b"Failed\n");
        return;
    }

    let len = string::strlen(text_ptr) as u32;
    if vfs::write(node, 0, len, text_ptr) < 0 {
        vga::puts(b"Write failed\n");
        return;
    }

    vga::puts(b"Wrote ");
    print_uint(len);
    vga::puts(b" bytes to ");
    vga::puts(&INPUT[filename_start..space_offset]);
    vga::putchar(b'\n');
}

unsafe fn cmd_save() {
    if INPUT_LEN <= 5 {
        vga::puts(b"Usage: save <filename>\n");
        vga::puts(b"Copies a ramfs file to the FAT16 disk.\n");
        return;
    }

    INPUT[INPUT_LEN] = 0;
    let filename = INPUT[5..INPUT_LEN].as_ptr();

    let node = vfs::finddir(VFS_ROOT, filename);
    if node.is_null() {
        vga::puts(b"File not found: ");
        vga::puts(&INPUT[5..INPUT_LEN]);
        vga::putchar(b'\n');
        return;
    }

    if (*node).size == 0 {
        vga::puts(b"File is empty\n");
        return;
    }

    let buf = heap::kmalloc((*node).size as usize);
    if buf.is_null() {
        vga::puts(b"Out of memory\n");
        return;
    }

    let n = vfs::read(node, 0, (*node).size, buf);
    if n <= 0 {
        vga::puts(b"Read error\n");
        heap::kfree(buf);
        return;
    }

    if fat16::write_file(filename, buf, n as u32) < 0 {
        vga::puts(b"Disk write failed\n");
        heap::kfree(buf);
        return;
    }

    heap::kfree(buf);
    vga::puts(b"Saved ");
    print_uint(n as u32);
    vga::puts(b" bytes to disk: ");
    vga::puts(&INPUT[5..INPUT_LEN]);
    vga::putchar(b'\n');
}

unsafe fn cmd_load() {
    if INPUT_LEN <= 5 {
        vga::puts(b"Usage: load <filename>\n");
        vga::puts(b"Copies a FAT16 disk file to ramfs.\n");
        return;
    }

    INPUT[INPUT_LEN] = 0;
    let filename = INPUT[5..INPUT_LEN].as_ptr();

    // Read from disk
    let max_size: u32 = 65536; // 64KB limit
    let buf = heap::kmalloc(max_size as usize);
    if buf.is_null() {
        vga::puts(b"Out of memory\n");
        return;
    }

    let n = fat16::read_file(filename, buf, max_size);
    if n < 0 {
        vga::puts(b"File not found on disk: ");
        vga::puts(&INPUT[5..INPUT_LEN]);
        vga::putchar(b'\n');
        heap::kfree(buf);
        return;
    }

    if n == 0 {
        vga::puts(b"File is empty\n");
        heap::kfree(buf);
        return;
    }

    // Create or find the file in ramfs
    let mut node = vfs::finddir(VFS_ROOT, filename);
    if node.is_null() {
        node = vfs::create_file(filename);
    }
    if node.is_null() {
        vga::puts(b"Cannot create ramfs file\n");
        heap::kfree(buf);
        return;
    }

    if vfs::write(node, 0, n as u32, buf) < 0 {
        vga::puts(b"Write to ramfs failed\n");
        heap::kfree(buf);
        return;
    }

    heap::kfree(buf);
    vga::puts(b"Loaded ");
    print_uint(n as u32);
    vga::puts(b" bytes from disk: ");
    vga::puts(&INPUT[5..INPUT_LEN]);
    vga::putchar(b'\n');
}

unsafe fn cmd_run() {
    if INPUT_LEN <= 4 {
        vga::puts(b"Usage: run <binary>\n");
        return;
    }

    INPUT[INPUT_LEN] = 0;
    let filename = INPUT[4..INPUT_LEN].as_ptr();

    let node = vfs::finddir(VFS_ROOT, filename);
    if node.is_null() {
        vga::puts(b"File not found: ");
        vga::puts(&INPUT[4..INPUT_LEN]);
        vga::putchar(b'\n');
        return;
    }

    if (*node).size == 0 {
        vga::puts(b"Empty file\n");
        return;
    }

    // Read file into a temporary buffer to check format
    let file_size = (*node).size;
    let buf = heap::kmalloc(file_size as usize);
    if buf.is_null() {
        vga::puts(b"Out of memory\n");
        return;
    }
    let n = vfs::read(node, 0, file_size, buf);
    if n <= 0 {
        vga::puts(b"Read error\n");
        heap::kfree(buf);
        return;
    }

    // Check if ELF or flat binary
    if n >= 4 && elf::is_elf(buf) {
        // ELF binary
        match elf::load(buf, n as u32) {
            Ok(info) => {
                heap::kfree(buf);
                syscall::set_brk(info.brk);
                let entry: extern "C" fn() = core::mem::transmute(info.entry as usize);
                let ret = recovery::run_protected(entry);
                if ret == -99 {
                    vga::puts(b"Program crashed\n");
                }
            }
            Err(msg) => {
                heap::kfree(buf);
                vga::puts(msg.as_bytes());
                vga::putchar(b'\n');
            }
        }
    } else {
        // Flat binary — load at CC_LOAD_BASE (legacy)
        let load_addr = rc::emit::CC_LOAD_BASE as *mut u8;
        string::memcpy(load_addr, buf, n as usize);
        heap::kfree(buf);
        let entry: extern "C" fn() = core::mem::transmute(load_addr);
        let ret = recovery::run_protected(entry);
        if ret == -99 {
            vga::puts(b"Program crashed (return -99)\n");
        }
    }
}

/// TCP interactive client.
/// Usage: tcp <ip> <port>
/// Connect to a server, send typed lines, display received data.
/// Empty line or Ctrl-D to close.
unsafe fn cmd_tcp() {
    if INPUT_LEN <= 4 {
        console_puts(b"Usage: tcp <ip> <port>\n");
        return;
    }

    INPUT[INPUT_LEN] = 0;

    // Find the space between host and port
    let arg_start = 4;
    let arg_ptr = INPUT[arg_start..].as_mut_ptr();
    let space = string::strchr(arg_ptr, b' ');
    if space.is_null() {
        console_puts(b"Usage: tcp <ip> <port>\n");
        return;
    }

    // Null-terminate the host part
    let space_offset = (space as usize) - (INPUT.as_ptr() as usize);
    INPUT[space_offset] = 0;

    let mut ip = parse_ip(arg_ptr);
    if ip == 0 {
        if dns::resolve(arg_ptr, &mut ip) == 0 {
            console_puts(b"Could not resolve host\n");
            return;
        }
    }

    let port = string::atoi(INPUT[space_offset + 1..].as_ptr()) as u16;
    if port == 0 {
        console_puts(b"Invalid port\n");
        return;
    }

    console_puts(b"Connecting...\n");
    let conn = tcp::connect(ip, port);
    if conn < 0 {
        console_puts(b"Connection failed\n");
        return;
    }
    console_puts(b"Connected. Empty line to close.\n");

    let mut line = [0u8; 128];
    let mut recv_buf = [0u8; 512];

    loop {
        // Drain any pending received data
        loop {
            let n = tcp::recv(conn, recv_buf.as_mut_ptr() as *mut u8, 511, 5);
            if n <= 0 {
                break;
            }
            recv_buf[n as usize] = 0;
            console_puts(&recv_buf[..n as usize]);
        }

        // Read a line from the user
        console_puts(b"> ");
        let mut line_len: usize = 0;
        loop {
            let c = console_getchar();
            if c == b'\n' || c == b'\r' {
                console_putchar(b'\n');
                line[line_len] = 0;
                break;
            }
            if c == 4 {
                // Ctrl-D
                line_len = 0;
                line[0] = 0;
                console_putchar(b'\n');
                break;
            }
            if (c == b'\x08' || c == 127) && line_len > 0 {
                line_len -= 1;
                console_putchar(b'\x08');
                continue;
            }
            if line_len < line.len() - 2 {
                line[line_len] = c;
                line_len += 1;
                console_putchar(c);
            }
        }

        if line_len == 0 && line[0] == 0 {
            break;
        }

        // Append newline and send
        line[line_len] = b'\n';
        line_len += 1;
        line[line_len] = 0;
        if tcp::send(conn, line.as_ptr() as *const u8, line_len as u16) < 0 {
            console_puts(b"Send failed\n");
            break;
        }
    }

    tcp::close(conn);
    console_puts(b"Connection closed.\n");
}

/// HTTP GET command.
/// Usage: http <ip|host> [path]
/// Connects to port 80, sends GET request, prints response.
unsafe fn cmd_http() {
    if INPUT_LEN <= 5 {
        console_puts(b"Usage: http <ip|host> <path>\n");
        return;
    }

    INPUT[INPUT_LEN] = 0;

    let arg_start = 5;
    let arg_ptr = INPUT[arg_start..].as_mut_ptr();
    let space = string::strchr(arg_ptr, b' ');

    let path: *const u8;
    if !space.is_null() {
        let space_offset = (space as usize) - (INPUT.as_ptr() as usize);
        INPUT[space_offset] = 0;
        path = INPUT[space_offset + 1..].as_ptr();
    } else {
        path = b"/\0".as_ptr();
    }

    let mut ip = parse_ip(arg_ptr);
    if ip == 0 {
        if dns::resolve(arg_ptr, &mut ip) == 0 {
            console_puts(b"Could not resolve host\n");
            return;
        }
    }

    console_puts(b"Connecting to ");
    let mut ip_str = [0u8; 16];
    net::ip_to_str(ip, ip_str.as_mut_ptr());
    console_puts(&ip_str);
    console_puts(b":80...\n");

    let conn = tcp::connect(ip, 80);
    if conn < 0 {
        console_puts(b"Connection failed\n");
        return;
    }

    // Build HTTP request
    let mut req = [0u8; 256];
    string::strcpy(req.as_mut_ptr(), b"GET \0".as_ptr());
    string::strcat(req.as_mut_ptr(), path);
    string::strcat(req.as_mut_ptr(), b" HTTP/1.0\r\nHost: \0".as_ptr());
    string::strcat(req.as_mut_ptr(), arg_ptr);
    string::strcat(req.as_mut_ptr(), b"\r\n\r\n\0".as_ptr());
    let req_len = string::strlen(req.as_ptr()) as u16;

    if tcp::send(conn, req.as_ptr() as *const u8, req_len) < 0 {
        console_puts(b"Send failed\n");
        tcp::close(conn);
        return;
    }

    // Read and print response until connection closes
    let mut recv_buf = [0u8; 512];
    loop {
        let n = tcp::recv(conn, recv_buf.as_mut_ptr() as *mut u8, 511, 300);
        if n <= 0 {
            break;
        }
        recv_buf[n as usize] = 0;
        console_puts(&recv_buf[..n as usize]);
    }
    console_putchar(b'\n');

    tcp::close(conn);
}

// ---------------------------------------------------------------------------
// Main shell loop -- never returns
// ---------------------------------------------------------------------------

/// Run the interactive shell. This function never returns.
pub unsafe fn run() -> ! {
    vga::puts(b"opsys v0.1\n");
    vga::puts(b"Type 'help' for available commands.\n\n");

    loop {
        shell_prompt();
        shell_readline();

        // Skip empty input (user just pressed Enter)
        if INPUT[0] == 0 {
            continue;
        }

        let input_ptr = INPUT.as_ptr();

        // Match against known commands
        if string::strcmp(input_ptr, b"help\0".as_ptr()) == 0 {
            cmd_help();
        } else if string::strcmp(input_ptr, b"clear\0".as_ptr()) == 0 {
            vga::clear();
        } else if string::strncmp(input_ptr, b"echo\0".as_ptr(), 4) == 0
            && (INPUT[4] == b' ' || INPUT[4] == 0)
        {
            cmd_echo();
        } else if string::strncmp(input_ptr, b"ls\0".as_ptr(), 2) == 0
            && (INPUT[2] == b' ' || INPUT[2] == 0)
        {
            cmd_ls();
        } else if string::strncmp(input_ptr, b"cat\0".as_ptr(), 3) == 0
            && (INPUT[3] == b' ' || INPUT[3] == 0)
        {
            cmd_cat();
        } else if string::strcmp(input_ptr, b"mem\0".as_ptr()) == 0 {
            cmd_mem();
        } else if string::strncmp(input_ptr, b"ping\0".as_ptr(), 4) == 0
            && (INPUT[4] == b' ' || INPUT[4] == 0)
        {
            cmd_ping();
        } else if string::strncmp(input_ptr, b"resolve\0".as_ptr(), 7) == 0
            && (INPUT[7] == b' ' || INPUT[7] == 0)
        {
            cmd_resolve();
        } else if string::strncmp(input_ptr, b"touch\0".as_ptr(), 5) == 0
            && (INPUT[5] == b' ' || INPUT[5] == 0)
        {
            cmd_touch();
        } else if string::strncmp(input_ptr, b"write\0".as_ptr(), 5) == 0
            && (INPUT[5] == b' ' || INPUT[5] == 0)
        {
            cmd_write();
        } else if string::strncmp(input_ptr, b"edit\0".as_ptr(), 4) == 0
            && (INPUT[4] == b' ' || INPUT[4] == 0)
        {
            if INPUT_LEN > 5 {
                INPUT[INPUT_LEN] = 0;
                editor::edit(INPUT[5..].as_ptr());
            } else {
                vga::puts(b"Usage: edit <filename>\n");
            }
        } else if string::strncmp(input_ptr, b"rc\0".as_ptr(), 2) == 0
            && (INPUT[2] == b' ' || INPUT[2] == 0)
        {
            if INPUT_LEN > 3 {
                INPUT[INPUT_LEN] = 0;
                rc::rc_compile(INPUT[3..].as_ptr(), core::ptr::null());
            } else {
                vga::puts(b"Usage: rc <file.rs>\n");
            }
        } else if string::strncmp(input_ptr, b"run\0".as_ptr(), 3) == 0
            && (INPUT[3] == b' ' || INPUT[3] == 0)
        {
            cmd_run();
        } else if string::strncmp(input_ptr, b"tcp\0".as_ptr(), 3) == 0
            && (INPUT[3] == b' ' || INPUT[3] == 0)
        {
            cmd_tcp();
        } else if string::strncmp(input_ptr, b"http\0".as_ptr(), 4) == 0
            && (INPUT[4] == b' ' || INPUT[4] == 0)
        {
            cmd_http();
        } else if string::strncmp(input_ptr, b"save\0".as_ptr(), 4) == 0
            && (INPUT[4] == b' ' || INPUT[4] == 0)
        {
            cmd_save();
        } else if string::strncmp(input_ptr, b"load\0".as_ptr(), 4) == 0
            && (INPUT[4] == b' ' || INPUT[4] == 0)
        {
            cmd_load();
        } else if string::strcmp(input_ptr, b"threads\0".as_ptr()) == 0 {
            cmd_threads();
        } else if string::strcmp(input_ptr, b"spawn\0".as_ptr()) == 0 {
            cmd_spawn();
        } else if string::strcmp(input_ptr, b"mirror\0".as_ptr()) == 0 {
            if vga::get_serial_mirror() {
                vga::set_serial_mirror(false);
                vga::puts(b"Serial mirror: OFF\n");
            } else {
                vga::set_serial_mirror(true);
                vga::puts(b"Serial mirror: ON\n");
            }
        } else if string::strcmp(input_ptr, b"reboot\0".as_ptr()) == 0 {
            cmd_reboot();
        } else {
            vga::puts(b"unknown command: ");
            vga::puts(&INPUT[..INPUT_LEN]);
            vga::putchar(b'\n');
        }
    }
}
