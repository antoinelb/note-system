#!/bin/sh
# The palette's "edit template" reaches the one editor from the table too
# (adr/2026-09-edit-template-reaches-the-logs-from-the-table.md): the
# picker stands over the table screen, and a pick switches to the logs and
# opens the template in the centre pane — the one full-page surface the
# shared editor has off the table.
#
# The vault is the oracle, never a pixel: what proves the template really
# opened in the editor is the sentence typed into it reaching
# templates/concept.typ on disk. Every keystroke that crosses an async
# focus grab — the screen switch, the palette, the picker, the i that
# enters insert mode — is paced (harness.sh `e2e_key_paced`).
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start

# onto the table, the screen the command used to be hidden on
e2e_key_paced ctrl+1

e2e_key_paced ctrl+p
e2e_type_paced "edit template"
e2e_key_paced Return

# the picker is up over the table: "concept" narrows to one row
e2e_type_paced "concept"
e2e_key_paced Return

# the pick landed on the logs with the template in the centre pane, the
# caret past its "= {{title}}" line as on any first open
# (adr/2026-09-a-note-reopens-where-it-was-left.md), so i appends there
# and disturbs nothing already in the file
e2e_key_paced i
e2e_type "edited from the table"
e2e_key Escape
e2e_note_holds "templates/concept.typ" "edited from the table"

# and the template is still a template: its own preamble stands
e2e_note_holds "templates/concept.typ" '#import "/templates/template.typ": *'

echo "ok: $(basename "$0")"
