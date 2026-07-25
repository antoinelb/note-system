.PHONY: init static test check-vault

VAULT := tests/fixtures/vault

init:
	git config core.hooksPath hooks

check-vault:
	@fails=0; \
	for f in $$(find $(VAULT) -name '*.typ' ! -name template.typ ! -path '*/.index/*'); do \
		typst compile -f pdf --root $(VAULT) $$f /dev/null || { echo "FAIL: $$f"; fails=$$((fails+1)); }; \
	done; \
	[ $$fails -eq 0 ] && echo "check-vault: all notes compile" || { echo "check-vault: $$fails failure(s)"; exit 1; }

static:
	cargo fmt --all
	cargo clippy --workspace --all-targets --all-features -- -D warnings

# 100% on all three axes; a shortfall on regions is diagnosed per instantiation
# before writing any test, see docs/adr/2026-07-couverture-100-pourcent-lignes.md
test:
	cargo +nightly llvm-cov \
	  --ignore-filename-regex '(lib\.rs|/mod\.rs|/main\.rs)$$' \
	  --fail-under-regions 100 \
	  --fail-under-lines 100 \
	  --fail-under-functions 100
