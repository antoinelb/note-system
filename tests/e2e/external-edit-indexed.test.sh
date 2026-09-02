#!/bin/sh
# A change made outside the app while it runs reaches the index through
# the watcher (adr/2026-07-incremental-vault-watching.md,
# adr/2026-08-watcher-feeds-the-ui.md): a link appended to an existing
# note by another program gains its links row, and a whole note dropped
# into permanent/ gains its notes row — with no keystroke sent to the
# window at all. The index is the oracle; the shipped binary's watcher
# thread and the feed into the shell are the only things that can fill it.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start

# the app builds its index from scratch on launch (the fixture's .index is
# removed by e2e_start); wait for that survey before editing under it, so
# the rows below can only come from the watcher
e2e_index_holds "SELECT 1 FROM notes WHERE id = 'plain-files';"

# an editor saving in place: plain-files.typ links nothing in the fixture
printf '\nAppended from outside the app: #l("luhmann")\n' \
    >> "$E2E_DIR/vault/permanent/plain-files.typ"
e2e_index_holds "SELECT 1 FROM links \
    WHERE source_path = 'permanent/plain-files.typ' AND target_id = 'luhmann';"

# a note written whole by another program, atomically as a program should
cat > "$E2E_DIR/vault/permanent/.outside.typ.tmp" <<'EOF'
#import "/templates/template.typ": *
#show: note
#meta(
  id: "from-outside",
  type: "idea",
  created: "2026-07-24",
  tags: ("external",),
)

= Written by another process
EOF
mv "$E2E_DIR/vault/permanent/.outside.typ.tmp" \
    "$E2E_DIR/vault/permanent/from-outside.typ"
e2e_index_holds "SELECT 1 FROM notes \
    WHERE id = 'from-outside' AND type = 'idea' AND category = 'permanent';"
e2e_index_holds "SELECT 1 FROM tags \
    WHERE note_path = 'permanent/from-outside.typ' AND tag = 'external';"

echo "ok: $(basename "$0")"
