# Seasons are school semesters: winter, summer, autumn

## Context

The logs screen shows three time scales side by side (day, week, season), and the season id form was frozen at `2026-summer` by the design's rail rows (`design/wireframes-v0.md` § The logs screen).
Nothing anywhere defined the boundaries — what dates `2026-summer` actually covers — and the scale chain (day → ISO week → season) cannot be computed without them.

## Decision

**A season is a school semester: winter = January–April, summer = May–August, autumn = September–December.**

Three seasons per year, no spring.
The wireframe's season row text "summer · spring · winter" is superseded mockup drift; the rail shows the three semester seasons.

Consequences:

- No season crosses New Year, so a season id is always `{calendar-year}-{name}` — the season function is a three-arm match on the month, with no year adjustment.
- The fixture id `2026-summer` covers May–August 2026 (`created: 2026-05-01`).

The same decision pins the remaining time-id details:

- Week ids use the **ISO week-year** prefix and a **zero-padded two-digit** week number: `2026-w05`, never `2026-w5` — lexicographic order in the index equals chronological order, the same property daily ids already have and prev/next SQL relies on.
- Around New Year the week prefix can differ from the calendar year: 2027-01-01 → `2026-w53`, 2024-12-30 → `2025-w01`; the day and season ids of those dates still use the calendar year.
- For the phase-7 rail, **a week belongs to the season of its Monday** (implemented in phase 7 — nothing in phase 5 consumes it).

## Alternatives rejected

- **Meteorological seasons (Dec–Feb winter, …)** — four seasons and a winter spanning New Year, which forces a year-labeling rule (`2026-winter` = Dec 2026 → Feb 2027) and a season/year mismatch every January; the user's mental year is structured by semesters, not by weather.
- **Calendar quarters (Jan–Mar, …)** — also avoids the year-span, but its four boundaries match nothing lived.
- **Astronomical seasons** — solstice/equinox dates shift every year and need an ephemeris table; the most code for the least benefit.
