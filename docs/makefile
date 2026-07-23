.PHONY: static test

static:
	cargo fmt --all
	cargo clippy --workspace --all-targets --all-features -- -D warnings

test:
	cargo +nightly llvm-cov --ignore-filename-regex '(lib\.rs|/mod\.rs|/main\.rs)$$'
