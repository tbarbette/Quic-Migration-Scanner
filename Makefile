.PHONY: build release test clean

build:
	./configure-quiche.sh
	cargo build

release:
	./configure-quiche.sh
	cargo build --release

test:
	./configure-quiche.sh
	cargo test

clean:
	cargo clean
