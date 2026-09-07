.PHONY: init static test check-vault e2e upgrade

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
	flock target/.test.lock cargo +nightly llvm-cov \
	  --ignore-filename-regex '(lib\.rs|/mod\.rs)$$' \
	  --fail-under-regions 100 \
	  --fail-under-lines 100 \
	  --fail-under-functions 100

# The shipped window, driven by keystrokes in a headless X server: the one
# layer `make test` cannot see, since dioxus_ssr renders markup and not a
# WebKitGTK surface (adr/2026-08-headless-x11-e2e.md). Release, because a
# debug typst compile turns every settle into a race. Not in the pre-commit
# hook: each scenario costs seconds, not milliseconds.
e2e:
	cargo build --release
	@fails=0; \
	for t in tests/e2e/*.test.sh; do \
		sh $$t || fails=$$((fails+1)); \
	done; \
	[ $$fails -eq 0 ] && echo "e2e: all scenarios passed" || { echo "e2e: $$fails failure(s)"; exit 1; }

# a persistent scratch vault outside the repo, seeded from the fixtures:
# running against the fixtures themselves pollutes canonical test data
# (adr/2026-07-dev-test-vault-locations.md). Notes persist across runs;
# the app seeds any missing template at startup from its embedded
# defaults, and in-app template edits persist
# (adr/2026-08-templates-seeded-from-embedded-fixtures.md).
DEV_VAULT := $(HOME)/.local/share/note-system/dev-vault

run:
	@mkdir -p $(dir $(DEV_VAULT))
	@test -d $(DEV_VAULT) || cp -r $(VAULT) $(DEV_VAULT)
	NOTE_VAULT=$(DEV_VAULT) cargo run

# the install is the release: the one gate the hook skips for speed runs
# here, before a binary starts being used for real notes
# (adr/2026-09-the-pre-commit-hook-is-the-ci.md)
upgrade: e2e
	cargo install --locked --force --path .
