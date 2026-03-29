KERNEL = target/x86-opsys/release/opsys
CARGO  = cargo

all: kernel.elf

kernel.elf: FORCE
	$(CARGO) +nightly build --release
	cp $(KERNEL) kernel.elf

debug: FORCE
	$(CARGO) +nightly build
	cp target/x86-opsys/debug/opsys kernel.elf

# Host tools
tools/mkinitrd: tools/mkinitrd.c
	gcc -o $@ $<

# Programs to include in initrd
PROGRAMS = $(wildcard programs/*.c) $(wildcard programs/*.h)

sysroot/hello.txt:
	mkdir -p sysroot
	echo "Hello from initrd!" > sysroot/hello.txt

sysroot/readme.txt:
	mkdir -p sysroot
	echo "opsys-rust v0.1 - a self-contained x86 operating system in Rust" > sysroot/readme.txt

sysroot/programs: $(PROGRAMS)
	mkdir -p sysroot
	@if ls programs/*.c >/dev/null 2>&1; then cp programs/*.c sysroot/; fi
	@if ls programs/*.h >/dev/null 2>&1; then cp programs/*.h sysroot/; fi

initrd.img: tools/mkinitrd sysroot/hello.txt sysroot/readme.txt sysroot/programs
	tools/mkinitrd initrd.img $(wildcard sysroot/*)

disk.img:
	dd if=/dev/zero of=disk.img bs=1M count=32
	mkfs.fat -F 16 disk.img

format-disk:
	rm -f disk.img
	dd if=/dev/zero of=disk.img bs=1M count=32
	mkfs.fat -F 16 disk.img

run: kernel.elf initrd.img
	@if [ -f disk.img ]; then \
		qemu-system-i386 -kernel kernel.elf -display curses \
			-device rtl8139,netdev=net0 -netdev user,id=net0 \
			-initrd initrd.img \
			-serial tcp::2324,server,nowait \
			-drive file=disk.img,format=raw,if=ide,index=0; \
	else \
		qemu-system-i386 -kernel kernel.elf -display curses \
			-device rtl8139,netdev=net0 -netdev user,id=net0 \
			-initrd initrd.img \
			-serial tcp::2324,server,nowait; \
	fi

test: kernel.elf initrd.img
	@echo "Running opsys compiler test suite..."
	@timeout 60 qemu-system-i386 -kernel kernel.elf -display none \
		-device rtl8139,netdev=net0 -netdev user,id=net0 \
		-initrd initrd.img -serial stdio -append "autotest" \
		-no-reboot 2>/dev/null | tee test_output.txt; \
	echo ""; \
	if grep -q "^FAIL" test_output.txt; then \
		echo "*** TESTS FAILED ***"; exit 1; \
	elif grep -q "=== TEST END ===" test_output.txt; then \
		echo "*** ALL TESTS PASSED ***"; \
	else \
		echo "*** TESTS DID NOT COMPLETE ***"; exit 1; \
	fi

clean:
	cargo clean
	rm -f kernel.elf tools/mkinitrd initrd.img test_output.txt
	rm -rf sysroot

FORCE:
.PHONY: all run test clean format-disk debug FORCE
