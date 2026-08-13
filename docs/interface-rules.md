# Antifragile Interface Rules (AIR) — v1.0

**Scope:** all interactive software I ship — web (HTML/JS/CSS, WASM), desktop (Rust, Python), mobile.
**Operating assumption:** the interface will be used by someone tired, cold, interrupted, on a bad
network, at 03:00, during a flood, to make a decision that matters. Design for that person. Everyone
else is a subset of that case.

**Status of this file:** it is a constraint set, not an aspiration. Every rule has a check. A rule
without a check is a slogan and gets deleted.

---

## 0. Terms — used precisely, not as marketing

| Term | Meaning | Design consequence |
|---|---|---|
| **Robust** [R] | Behaviour is unchanged under stress. | Fixed budgets, fixed layout, no adaptive surprises. |
| **Resilient** [S] | Function is restored after stress. | Degraded modes, undo, state recovery. |
| **Antifragile** [A] | The system is *better after* stress than before. | Error harvesting, drills, incident→test→rule loop. |

Most "antifragile design" writing is actually about robustness. Robustness and resilience are the
floor here, not the goal. The genuinely antifragile part of this document is §K and §L: the
mechanisms by which every failure, near-miss and user correction permanently improves the system.
**If you implement §A–J and skip §K–L, you have built a robust system that will slowly rot.**

### What this document rejects

- **"Errors and latency can't happen."** They will. The design goal is that they are *bounded,
  *detectable*, *attributable* and *recoverable*. A system built on the premise that failure is
  impossible fails silently and catastrophically; a system built on the premise that failure is
  routine fails loudly and locally.
- **Adaptive/personalised UI for "user levels."** Mode-dependent layouts are the single most
  reliable source of human error in high-consequence systems. Students and engineers get the *same
  screen*; they get *different depth in the same place* (§F).
- **Confirmation dialogs as a safety mechanism.** Under stress, operators click through them. Undo
  and delay-with-cancel are real; "Are you sure?" is theatre (§E).
- **Beauty as a proxy for quality.** Aesthetic polish is worth having and worth zero when the gauge
  stopped reporting 40 minutes ago and the dashboard is still drawing a line.

---

## The Core Ten (memorise these; the rest is elaboration)

1. **The interface never lies about what it knows.** Every number carries age, source, and precision.
2. **Absence is not zero. Stale is not current. Model is not observation.** Render the difference.
3. **Acknowledge every input within 100 ms, regardless of backend state.** Always.
4. **Read-only function survives total loss of network and server.** Local-first or it doesn't ship.
5. **Nothing on screen moves unless the user moved it.** Detail expands in place.
6. **Every action is undoable, or it is typed-confirmed and logged. Nothing in between.**
7. **Every alert is actionable, prioritised, rate-limited, and persists until dismissed.**
8. **Errors state: what happened, what's affected, what to do now, and a copyable ID.**
9. **Every export carries its own provenance.** Screenshots get forwarded; they must self-describe.
10. **Every incident produces a regression test.** No exceptions, no "it was a one-off."

---

## A. TRU — Truth and provenance

The interface is an instrument. An instrument that reads plausibly when disconnected is worse than
one that reads nothing.

**TRU-1 [R]. Every displayed value carries, or is one interaction away from, its provenance:**
source, acquisition/computation time, and processing lineage (raw / QC'd / gap-filled / modelled).
*Why:* during an event, "where does this number come from" is asked constantly and answered wrongly.
*Check:* pick any number on any screen; provenance is reachable in ≤1 interaction, without leaving
the screen.

**TRU-2 [R]. Data age is always visible for anything time-varying.** Not "updated 2 hours ago" alone
— absolute timestamp with timezone must be co-located or immediately adjacent.
*Check:* screenshot any live view; the absolute as-of time is legible in the image.

**TRU-3 [R]. Staleness is a rendered state, not a footnote.** At `age > 2× expected update interval`,
the value is visually marked stale. At `age > 5×`, the value is **replaced** by an explicit
unavailable state — do not keep drawing the last known value.
*Why:* the classic operational failure is a frozen gauge presented as a live one.
*Check:* freeze a feed in a test harness; assert the visual state transitions at both thresholds.

**TRU-4 [R]. Gaps render as gaps.** Never interpolate, smooth, forward-fill or connect across missing
data without an explicit, visually distinct treatment and a legend entry.
*Check:* inject a 6-hour hole in a series; the plotted line breaks.

**TRU-5 [R]. Observation, analysis, and forecast are permanently distinguishable** by a
non-colour channel (stroke style, hatch, background band, explicit "now" divider) — not by legend
alone and not by colour alone.
*Check:* render in greyscale; the boundary is still obvious.

**TRU-6 [R]. Displayed precision is derived from declared instrument/model precision**, never from
the language's default float formatting.
*Why:* `1247.8391 m³/s` asserts a level of knowledge that does not exist and is trusted anyway.
*Check:* no numeric output path uses bare `{}`/`str()`/`toString()`; all go through a
precision-aware formatter that takes the quantity's declared significant figures.

**TRU-7 [R]. Uncertainty is available for every forecast quantity in one interaction**, and a
deterministic single line is never the *only* representation offered.
*Check:* every forecast view has an ensemble/interval toggle or overlay reachable without navigation.

**TRU-8 [R]. Empty, single-point, all-NaN, all-identical, and out-of-range datasets each have a
designed rendering.** None of them may produce a blank panel, a crash, or an axis from 0 to 0.
*Check:* the five degenerate fixtures are in the visual test suite for every chart component.

---

## B. TIM — Time, units, identity

Unit and time-convention errors are the cheapest catastrophic bugs available. They are also fully
preventable by type discipline.

**TIM-1 [R]. Store and compute in UTC; display in an explicitly labelled timezone.** The label is
always rendered. Never display a naive datetime.
*Check:* grep the codebase for datetime formatting without a tz token — zero results.

**TIM-2 [R]. Interval conventions are stated in the UI, not just the docs.** Hour-ending vs
hour-beginning, instantaneous vs accumulated, local civil time vs standard time (no DST).
*Why:* a 1-hour offset in a flood forecast is a wrong decision, not a rounding issue.
*Check:* every time-series axis or table header states its convention.

**TIM-3 [R]. Units are displayed adjacent to every value, always.** No unit is ever implied by
context, column position, or user setting alone.
*Check:* no bare numbers in any critical readout.

**TIM-4 [R]. Units are types at the boundary.** Parse into a typed quantity at ingest; convert only
through explicit, tested conversions; never convert silently for display without showing both the
value and the unit.
*Check:* the domain layer contains no bare `f64`/`float` for physical quantities.

**TIM-5 [R]. Every entity has one canonical identifier displayed alongside its human name.**
Station name + station code. Basin name + ID. Model run name + run ID.
*Why:* names collide, get renamed, and are transcribed wrongly over radio and phone.
*Check:* any entity referenced in an export can be resolved unambiguously from the export alone.

**TIM-6 [R]. Clock skew is detected and surfaced.** If client and server time differ by more than the
smallest data interval, say so.
*Check:* inject 10-minute skew; a persistent banner appears.

---

## C. LAT — Latency and responsiveness

Separate *acknowledgement* from *result*. The first is a hard contract; the second is best-effort.

**LAT-1 [R]. Any user input produces a visible state change within 100 ms**, on the slowest supported
device, under the worst supported network, at p99. This budget is never spent on network I/O.
*Check:* automated interaction-latency test under injected 3 s RTT; p99 ≤ 100 ms.

**LAT-2 [R]. Budgets:** 16 ms per frame for anything dragging/animating; 100 ms acknowledgement;
1 s for local operations or determinate progress appears; 10 s absolute ceiling before the operation
must offer cancel, elapsed time, and partial results.
*Check:* measured in CI on the reference low-end device profile, reported as p50/p95/p99 — never mean.

**LAT-3 [R]. The main/UI thread never blocks on I/O, parsing, or computation > 16 ms.** Off-thread,
worker, async, or chunked — no exceptions.
*Check:* long-task instrumentation in dev builds fails the build above threshold.

**LAT-4 [R]. No indeterminate spinner without an elapsed-time counter and a cancel affordance.**
*Why:* an operator cannot distinguish "slow" from "dead," and will reload, duplicating the request.
*Check:* zero bare spinners in the component inventory.

**LAT-5 [R]. Skeleton screens are forbidden for data-bearing regions.** They fabricate the shape of
content that may not exist. Use an explicit "loading — as of unknown" state.
*Check:* no skeleton components in critical data views.

**LAT-6 [R]. Stale-while-revalidate is the default read pattern**: show last known value marked with
its age, refresh underneath, never blank the screen to fetch.
*Check:* toggle network off mid-session; no view goes empty.

**LAT-7 [R]. Auto-refresh never moves anything under the pointer or steals focus**, never scrolls,
never reorders a list the user is reading. New data arrives in place or behind a "N new" affordance.
*Check:* scripted test — cursor over a row, refresh fires, row identity and position unchanged.

**LAT-8 [R]. Animation on any critical path is ≤ 200 ms and respects reduced-motion.** Decorative
animation on operational screens is a defect.

---

## D. DEG — The degradation ladder

Degraded modes are *designed*, *named*, *visible*, and *routinely exercised*. Undesigned degradation
is just failure with extra steps.

**DEG-1 [S]. Define and implement explicit tiers.** Baseline ladder:

| Tier | Available | Trigger |
|---|---|---|
| **0 — Full** | Everything | Normal |
| **1 — Reduced** | Read + cached compute; writes queued | Backend slow/partial |
| **2 — Local** | Last-known data, local models, local export | Network lost |
| **3 — Cold** | Static snapshot bundle, opens without any server | App/server unavailable |
| **4 — Paper** | Printable one-page situation sheet | Device/power loss |

**DEG-2 [R]. The current tier is displayed persistently** in the same place, in every tier, with the
same visual grammar. Tier changes are announced once, non-modally, and persist in a status area.
*Check:* screenshot in each tier; tier is identifiable from the image alone.

**DEG-3 [R]. Read-only core function works at Tier 2 with zero network.** This is a shipping gate,
not a nice-to-have.
*Check:* CI runs the smoke suite with the network namespace disabled.

**DEG-4 [S]. Writes made in a degraded tier are queued, visible, counted, and individually
inspectable**, with an explicit, operator-visible conflict resolution on reconnect. Never
auto-merge silently.
*Check:* offline-edit → reconnect-with-conflict test exists and asserts the user is asked.

**DEG-5 [A]. Every degraded path is exercised at least every 90 days** in a drill using a real task.
Untested fallbacks are decorative.
*Why:* this is where antifragility actually lives — the fallback path improves because it is used
under mild stress regularly, instead of failing under severe stress once.
*Check:* drill log in the repo, with date, tier exercised, and defects found.

**DEG-6 [R]. Tier 3 artifact is a single self-contained file** (HTML with inlined assets, or a signed
bundle) that opens from a USB stick with no install, no network, no login.

---

## E. ACT — Action safety and reversibility

**ACT-1 [R]. Classify every action: reversible / delayed / irreversible.** The class determines the
safety mechanism; there is no fourth option and no unclassified action.

**ACT-2 [R]. Reversible actions execute immediately with undo.** No confirmation dialog.
*Why:* dialogs on safe actions train click-through, which then defeats dialogs on unsafe ones.

**ACT-3 [R]. Delayed actions execute after a visible countdown with cancel** (5–10 s). Preferred over
confirmation wherever the action is not instantaneous in effect.

**ACT-4 [R]. Irreversible actions require typing a specific non-generic token** (the entity's
identifier, not "yes"), state exactly what will be destroyed and what cannot be recovered, and are
logged with actor, timestamp, and the state before.
*Check:* no `confirm()`-equivalent guards an irreversible action.

**ACT-5 [R]. Destructive and irreversible controls are never adjacent to frequent controls**, never
on the primary action position, and never reachable by the same keyboard sequence as a common action.

**ACT-6 [R]. No action is triggered by hover, focus, or scroll.** Explicit activation only.

**ACT-7 [R]. Operator input is never lost.** Persist locally within 500 ms of an input pause; restore
on crash, reload, tab close, and app kill.
*Check:* kill -9 mid-form; relaunch restores the field contents.

**ACT-8 [R]. Idempotency on every write.** Client-generated request ID; a retried submit never
duplicates. Operators under stress double-click and reload.
*Check:* fire the same submit twice; one record results.

---

## F. LAY — Layout constancy and dual-audience density

The student/engineer problem is solved by **layered depth in a fixed frame**, not by modes or roles.

**LAY-1 [R]. Spatial constancy: a given kind of information always occupies the same region**, across
sessions, users, tiers, and window sizes (subject to a defined responsive ladder). Muscle memory is a
safety feature.
*Check:* overlay screenshots from two sessions/users; critical regions align.

**LAY-2 [R]. Progressive disclosure expands in place. Nothing already visible relocates.** Expanding
a detail panel may push content down; it may never reorder, resize, or re-flow the primary readouts.
*Check:* record positions of the top-level readouts; expand every disclosure; positions unchanged.

**LAY-3 [R]. Defaults are the expert-safe answer.** Novice support is added *explanation*, never
*altered behaviour*, altered defaults, or a reduced action set.
*Why:* if the novice view were safe enough for a novice, it is the view the expert should see too;
if it is not, teaching it is malpractice.

**LAY-4 [R]. Explanations are in-place, on-demand, dismissible, and never block.** No tours, no modal
onboarding, no "tips" that occlude data.

**LAY-5 [R]. Expert accelerators are additive, never exclusive**: keyboard shortcuts, command
palette, direct numeric entry, URL/deep-link state, copy-as-code. Every one of them has a
discoverable pointer/touch equivalent.

**LAY-6 [R]. Modes are avoided; where unavoidable, a mode is (a) globally visible via persistent
chrome change, (b) hard to enter accidentally, (c) exited by Escape.**
*Check:* enumerate modes in the design doc; if the list is longer than 3, redesign.

**LAY-7 [R]. Critical actions are never behind a hamburger, overflow menu, hover reveal, or
horizontal scroll.**

**LAY-8 [R]. One screen answers "what is the situation right now" without scrolling or navigation**,
at the smallest supported viewport.
*Check:* the situation view fits 360×640 with all critical readouts legible.

**LAY-9 [R]. Information density is user-controlled by an explicit, persisted preference** (compact /
comfortable), which changes spacing and optional columns only — never which controls exist or where
regions live.

---

## G. ALR — Alerts and notifications

Adapted from alarm-management practice (ISA-18.2 / EEMUA 191 in spirit; the numbers below are design
targets for this context, not compliance claims).

**ALR-1 [R]. Every alert has a defined operator response.** If there is no action the operator can
take, it is not an alert — it is a log entry.
*Check:* the alert catalogue lists a response for each alert type; empty responses fail review.

**ALR-2 [R]. Three priorities, maximum**, with distinct visual *and* non-visual (position, sound,
persistence) treatment. Priority is defined by consequence × time-to-act, and is documented per
alert type.

**ALR-3 [R]. Rate limiting and flood suppression are mandatory.** Design targets: sustained
≤ 1 alert per 10 min per operator; burst ≤ 10 per 10 min, after which alerts of the same class
collapse into a single counted, expandable group.
*Why:* alarm floods are the canonical emergency-operations failure; unbounded alerting is a defect.
*Check:* inject 500 simultaneous threshold crossings; the UI remains usable and the count is exact.

**ALR-4 [R]. Critical alerts persist until explicitly acknowledged.** Toasts, snackbars and
auto-dismissing banners are forbidden for anything above the lowest priority.

**ALR-5 [R]. Acknowledgement is recorded** (who, when) and never bulk-clears more than the visible,
enumerated set.

**ALR-6 [R]. No alert modal ever occludes live data.** Alerts occupy a reserved region; they do not
take over the screen.

**ALR-7 [A]. Every alert type carries a nuisance metric** (fired / actioned ratio). Types below an
agreed action rate are re-tuned or deleted at each review. Alerting quality improves monotonically or
someone owns the exception.

---

## H. ERR — Errors and recovery

**ERR-1 [R]. Every user-visible error states five things:** what happened (plain language), what the
system did about it, what data is affected or missing, what the user should do now, and a copyable
diagnostic ID.
*Check:* no error string in the codebase lacking a "what to do now" clause.

**ERR-2 [R]. Nothing fails silently.** Every caught exception either resolves the user's intent or
surfaces. `catch {}` with an empty body fails review.

**ERR-3 [R]. Technical detail is always one click away, never the primary message**, and is
copyable as a block including app version, data hashes, tier, and recent action trace.

**ERR-4 [R]. Retries are bounded, backed off, and visible.** State the attempt count and stop.
Infinite silent retry is a defect, not resilience.

**ERR-5 [R]. Partial failure renders partially.** One failed data source degrades one region, marked;
it never blanks the view or blocks the rest.
*Check:* fail one of N sources; N−1 regions remain fully functional.

**ERR-6 [R]. Errors never destroy operator work**, never clear a form, never reset navigation state,
never lose scroll position or selection.

**ERR-7 [R]. The error message text is treated as UI and is reviewed as UI** — versioned, tested,
and translated where the app is bilingual. FR/EN parity is a shipping gate for anything used in
Quebec operations.

---

## I. INP — Input robustness and access-as-robustness

Accessibility here is not compliance; it is the same requirement as gloves, rain, glare, one hand,
and stress-induced tunnel vision.

**INP-1 [R]. Touch targets ≥ 48 dp; primary operational actions ≥ 56 dp; ≥ 8 dp separation.**
Field/gloved use raises the primary minimum to 64 dp.

**INP-2 [R]. Contrast ≥ 4.5:1 for text, ≥ 3:1 for UI boundaries; ≥ 7:1 for critical readouts**
(sunlight, projection, aging eyes).
*Check:* automated contrast audit in CI; zero violations.

**INP-3 [R]. Never encode meaning in colour alone**, and never rely on red/green discrimination.
Add shape, position, pattern, or label.
*Check:* greyscale + deuteranopia simulation of every critical view; all distinctions survive.

**INP-4 [R]. Every action is keyboard-reachable; focus order is logical; focus is always visible.**
No keyboard traps. Escape always backs out.

**INP-5 [R]. No hover-only affordance exists anywhere.** Touch devices and stressed users don't hover.

**INP-6 [R]. All numeric input accepts pasted, comma-decimal, space-grouped, and unit-suffixed
values**, and echoes the parsed interpretation before commit.
*Why:* FR-CA decimal commas and copy-paste from spreadsheets are the normal case, not the edge case.
*Check:* `1 234,56 m³/s` parses; the echoed interpretation is shown.

**INP-7 [R]. Input validation is inline, immediate, non-blocking, and never clears the field.**
Reject on commit, not on keystroke.

**INP-8 [R]. Text scales to 200% without loss of content or function**, and the layout survives
system font-size overrides.

**INP-9 [R]. All primary flows are operable one-handed on a phone**, thumb-reachable, in portrait.

---

## J. BLD — Build, supply chain, and delivery

A tool that fails because a third party is down during a flood is fragile regardless of how it looks.

**BLD-1 [R]. Zero runtime third-party dependencies for core function.** No CDN fonts, scripts,
styles, tiles, or analytics on the critical path. Vendor everything.
*Check:* run with all non-origin domains blocked; core function unaffected.

**BLD-2 [R]. Cold start to first meaningful readout ≤ 3 s on the reference low-end device**, from
local cache, with no network.

**BLD-3 [R]. Installable and runnable offline** (PWA with a real offline strategy, or a native
binary, or a self-contained file). Installation never requires a network round trip to a vendor.

**BLD-4 [R]. Deterministic builds; version, build hash, and data-snapshot hash are visible in the
UI** and included in every export and every diagnostic block.

**BLD-5 [R]. Dependency budget is explicit and justified.** Each runtime dependency has a named
owner-of-risk and a stated removal plan if unmaintained. Prefer fewer, boring, vendorable
dependencies over convenience.

**BLD-6 [R]. The app degrades across browser/OS versions rather than refusing to run.** Feature
detection, never version sniffing; a hard "unsupported" screen is a last resort and always offers
Tier 3.

**BLD-7 [R]. Rollback is a one-command operation and is tested**, including data-format rollback.

---

## K. OBS — Observability and the learning loop  *(this is the antifragile engine)*

**OBS-1 [A]. Every error, timeout, degraded-tier transition, retry, and validation rejection is
logged with enough context to reproduce**: version, tier, device class, network class, data hashes,
last N user actions.

**OBS-2 [A]. Every user *correction* is logged as a first-class signal.** A changed default, an
overridden value, an immediately-reversed action, a re-run with different parameters, a manual
recompute — each one is evidence that the interface guessed wrong.
*Why:* corrections are the highest-value telemetry that almost nobody collects. Errors tell you what
broke; corrections tell you what was subtly wrong the whole time.
*Check:* the correction stream exists and is reviewed on a fixed cadence.

**OBS-3 [A]. Recurring corrections become changed defaults.** If ≥ 30% of uses of a default override
it in the same direction, the default is wrong; change it or document why not.

**OBS-4 [A]. Near-misses are captured deliberately.** An always-available, one-interaction "this
was confusing / this nearly went wrong" affordance that captures full state automatically and asks
the user for one sentence. No form, no triage friction.
*Why:* in high-consequence domains, near-misses outnumber incidents by orders of magnitude and are
the cheapest possible learning substrate. Systems that only learn from incidents learn slowly and
expensively.

**OBS-5 [A]. Latency, tier-transition frequency, alert nuisance rate, and correction rate are
reviewed on a fixed cadence** with a named owner. Metrics nobody reads are logging, not observability.

**OBS-6 [R]. Telemetry degrades gracefully and never affects function.** Buffer locally, drop on
overflow, never block, never retry into the foreground, never send on the critical path.

**OBS-7 [R]. Operational telemetry is separable from personal data** and the retention policy is
stated. For institutional deployment, assume the data is subject to access requests.

---

## L. TST — Stress testing and definition of done

**TST-1 [A]. The demo dataset is never the test dataset.** Standard adversarial fixtures, required
for every data-bearing component:
empty · single point · two identical points · all NaN · leading/trailing gaps · interior gap ·
step change · negative where physically impossible · 100× expected magnitude · 10⁶ points ·
duplicate timestamps · non-monotonic timestamps · DST transition · leap day · unit mismatch at ingest.

**TST-2 [A]. CI runs the interaction suite under injected adversity:** RTT p50 200 ms / p99 3 s,
2% packet loss, 10-minute clock skew, one data source hard-down, one data source returning stale,
one returning garbage, CPU throttled 4×.
*Why:* a system that only meets its budgets on a fast laptop has never been tested.

**TST-3 [A]. Chaos is scheduled, not incidental.** A recurring exercise kills a dependency in a
staging environment during a realistic task, and the findings are written down.

**TST-4 [R]. Visual regression tests exist for: every tier, greyscale, 200% text, deuteranopia,
360 px width, and each adversarial fixture.**

**TST-5 [A]. Every field incident and every near-miss produces a regression test before the fix
merges.** The test is named with the incident ID.

**TST-6 [A]. Every incident is triaged against this document:** it either (a) violated an existing
rule → fix and strengthen the check, or (b) revealed a missing rule → add it, with the incident cited,
or (c) is explicitly declined with a written reason. All three outcomes are recorded.

**Definition of done — no feature ships without all of these:**

- [ ] p99 acknowledgement ≤ 100 ms under injected 3 s RTT
- [ ] Functions at Tier 2 (no network) for all read paths
- [ ] All 15 adversarial fixtures render correctly
- [ ] Every value shows age, unit, and provenance
- [ ] Every action classified and given its matching safety mechanism
- [ ] Every error message has the five parts (ERR-1), in FR and EN
- [ ] Keyboard-complete; greyscale-legible; 48 dp targets; 7:1 on critical readouts
- [ ] Layout positions unchanged by any disclosure or refresh
- [ ] Exports carry full provenance
- [ ] Telemetry emits errors and corrections for the new paths
- [ ] Rollback tested

---

## M. EXP — Exports and artifacts that leave the system

During an event, screenshots and PDFs get forwarded by email, printed, and shown in meetings hours
later, stripped of all context. Assume every artifact will be misread out of context unless it
defends itself.

**EXP-1 [R]. Every export (image, PDF, CSV, print) embeds:** generation time with tz, data as-of
time, source, model run ID, app version + build hash, and the tier it was produced in.

**EXP-2 [R]. Exports produced in a degraded tier are visibly marked as such** on the artifact itself.

**EXP-3 [R]. CSV exports carry a header block with units, conventions, missing-value sentinel, and
provenance**, and use unambiguous ISO-8601 timestamps with offset.

**EXP-4 [R]. There is a print stylesheet / print path for the situation view**, fitting one page.
That is Tier 4.

---

## N. Governance — how this document stays true

**GOV-1 [A]. Project-level rules may only *tighten* AIR, never loosen it.** Each project keeps a
`PROJECT-AIR.md` containing only additions and tightenings.

**GOV-2 [A]. Deviations require a written waiver** naming: the rule, the risk accepted, the
compensating control, the owner, and an **expiry date**. Waivers expire; they do not lapse into
custom. An expired waiver blocks release.

**GOV-3 [A]. Rules are added only with a citing incident, near-miss, or external evidence.** No
speculative rules — that is how rulesets bloat into documents nobody reads.

**GOV-4 [A]. A rule that becomes structurally impossible to violate** (enforced by the type system,
a lint, or a CI gate) **moves to Appendix "Automated"** and stops consuming human attention.
*This is the ratchet:* human vigilance is a scarce, degrading resource; every rule promoted to
automation frees capacity for the next failure mode. The document should shrink in the part humans
must remember, and grow in the part machines enforce.

**GOV-5 [A]. Version and date every change; keep the incident that caused it in the changelog.**

---

## Appendix 1 — Wiring this into Claude Code / LLM agents

Put in `CLAUDE.md` at repo root:

```markdown
# UI/UX constraints
This project follows AIR (see AIR-antifragile-interface-rules.md). Non-negotiable subset:

- No value renders without age, unit, and provenance. Stale data is marked, then withdrawn.
- Gaps render as gaps. Never interpolate or forward-fill for display.
- 100 ms acknowledgement budget for every input; never block the UI thread > 16 ms.
- Read paths must work with zero network, from local cache.
- Nothing already on screen may move as a result of refresh or disclosure.
- Every action is reversible-with-undo, delayed-with-cancel, or typed-confirmed. Nothing else.
- Errors state: what happened / what the system did / what's affected / what to do now / copyable ID.
- No colour-only encoding; no hover-only affordances; no auto-dismissing critical alerts;
  no skeleton screens; no bare spinners; no infinite retry; no empty catch blocks.
- All datetimes tz-aware; all physical quantities typed with units at the boundary.
- Every new data-bearing component ships with the 15 adversarial fixtures.

When a request conflicts with these, say so and propose the compliant alternative
instead of implementing the request. Cite the rule ID.
```

Review prompt for generated UI:

> Review this against AIR. For each of TRU/TIM/LAT/DEG/ACT/LAY/ALR/ERR/INP/BLD, list violations with
> rule IDs and the minimal fix. Do not comment on aesthetics. If a rule is not applicable, say so
> explicitly rather than skipping it.

## Appendix 2 — Stack notes

| Concern | Web (JS/WASM) | Rust desktop | Python desktop | Mobile |
|---|---|---|---|---|
| 16 ms budget | Web Workers; WASM off main thread | keep render loop free; `tokio` for I/O | Qt/GTK: never block the event loop; worker threads | main thread discipline |
| Offline core | Service worker + IndexedDB/OPFS | embedded store (SQLite/sled) | SQLite + local cache dir | on-device store first |
| Units as types | branded types in TS; newtypes in Rust→WASM | `uom` or newtype wrappers | `pint`, or NewType + validation at boundary | mirror the core lib |
| Determinism | pinned lockfile, vendored assets | `Cargo.lock`, `--locked` | lockfile + hashes, no `latest` | reproducible bundle |
| Tier 3 artifact | single inlined HTML | static export or embedded viewer | frozen HTML report | share-to-file |

---

*AIR v1.0 — changelog begins here. Every subsequent entry cites the incident or near-miss that
caused it.*
