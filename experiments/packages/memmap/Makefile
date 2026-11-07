CC ?= cc
CFLAGS ?= -O2 -g
WARN = -std=c11 -Wall -Wextra -Werror
PLENTY ?= plenty

.PHONY: all check check-native bench clean
all: target/libmemmap.so

target:
	mkdir -p target

target/libmemmap.so: native/memmap.c native/memmap.h | target
	$(CC) $(WARN) $(CFLAGS) -fPIC -shared native/memmap.c -o $@

target/test-native: native/memmap.c native/test_memmap.c native/memmap.h | target
	$(CC) $(WARN) $(CFLAGS) native/memmap.c native/test_memmap.c -o $@

check-native: target/test-native
	./target/test-native

check: all check-native
	$(PLENTY) check
	$(PLENTY) test --no-capture
	$(PLENTY) test --fast-compile --no-capture

target/bench: native/memmap.c native/bench.c native/memmap.h | target
	$(CC) $(WARN) $(CFLAGS) native/memmap.c native/bench.c -o $@

bench: target/bench
	./target/bench

clean:
	rm -rf target
