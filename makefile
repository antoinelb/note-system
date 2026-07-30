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

test:
	cargo +nightly llvm-cov \
	  --ignore-filename-regex '(lib\.rs|/mod\.rs|/main\.rs)$$' \
	  --fail-under-regions 100 \
	  --fail-under-lines 100 \
	  --fail-under-functions 100

# a persistent scratch vault outside the repo, seeded from the fixtures:
# running against the fixtures themselves pollutes canonical test data
# (adr/2026-07-dev-test-vault-locations.md). Notes persist across runs;
# the app-owned templates refresh every run so template changes propagate.
DEV_VAULT := $(HOME)/.local/share/note-system/dev-vault

run:
	@mkdir -p $(dir $(DEV_VAULT))
	@test -d $(DEV_VAULT) || cp -r $(VAULT) $(DEV_VAULT)
	@cp $(VAULT)/templates/*.typ $(DEV_VAULT)/templates/
	NOTE_VAULT=$(DEV_VAULT) cargo run
