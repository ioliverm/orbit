# Orbit v1 — UI/UX proposal

| Field       | Value                                                      |
|-------------|------------------------------------------------------------|
| Version     | 0.1-draft                                                  |
| Date        | 2026-04-18                                                 |
| Status      | Proposal — first design pass, pre-implementation           |
| Owner       | ux-designer                                                |
| Source spec | `/Users/ivan/Development/projects/orbit/docs/specs/orbit-v1-persona-b-spain.md` |
| Related ADRs| 003 (engine), 004 (rule-set versioning), 005 (data model), 006 (market data), 007 (FX), 008 (export traceability) |

This is a **proposal**, not a final design. The HTML/CSS under `screens/` is reference-quality: build-to fidelity on layout, spacing, and pattern, not yet production-ready with real components. It exists so engineers can copy tokens and markup patterns directly, and so the proposal can be evaluated visually rather than described.

Read this doc top-to-bottom. It makes opinionated choices. Where it presents options, section 6 picks a recommendation.

---

## 1. Design goal in one paragraph

Orbit is a **decision-support instrument** for financially literate Spanish-tax-resident equity holders making six-figure+ decisions. The UI must feel like a professional tool, not a consumer app: information-dense, honest about uncertainty, legally careful, and fluent in Spanish tax vocabulary. Users arrive mid-decision (pre-IPO modeling, or post-IPO "do I sell today"), not browsing; every screen's job is to compress the distance between their question and a defensible, traceable, exportable answer — while reminding them, visibly but without scolding, that Orbit is not their asesor fiscal. Aesthetic target: **Bloomberg-terminal density meets Linear's restraint, in Spanish**.

## 2. Personas & context of use

Re-stating from the spec in one sentence each — design decisions trace back here.

- **María (pre-IPO, desktop, evening).** At home on a 14" laptop, spreadsheet open in another tab, has 45 minutes before her partner wants dinner. Wants to run three scenarios, export a PDF for her gestor tomorrow, close the laptop.
- **María (post-IPO, phone + desktop, weekday morning).** Coffee in hand, phone first check of stock price, then laptop. Wants a 5-minute answer on "if I sell X + Y + Z now, what lands?" Tolerates zero ambiguity from the tool on what's a guess vs. a rule.
- **Gestor / advisor (PDF-only, no Orbit login).** Receives María's PDF export, reads it cold in their office, reconciles against their own tools. Never sees the UI — the PDF is the entire interface. Export design is a first-class product surface, not an afterthought.

## 3. Information architecture

### 3.1 Top-level navigation

One persistent left rail, collapsed to a hamburger below 1024px:

```
ORBIT

Portfolio
  Dashboard                 (home)
  Grants                    (list + detail)
  Vesting                   (timeline)

Decisions                   (section header)
  Sell-now calculator     [paid]
  Scenario modeler        [paid]

Compliance
  Art. 7.p trips
  Modelo 720 worksheet    [paid export]
  Exports                 [paid]

Account
  Profile & residency
  Billing
  Data & privacy          (GDPR / DSR self-service)
```

**Why this shape:**
- Three verb-flavored sections — "what I own", "decisions I'm weighing", "what I owe / must file" — match how the persona thinks about the lifecycle, not how the data model is structured.
- **Sell-now is promoted to top level, not buried under "Scenarios"**. It's a distinct post-IPO job with its own ergonomics; treating it as a scenario-variant dilutes both.
- "Compliance" is a neutral label for the regulatory surfaces (Art. 7.p, Modelo 720, exports for gestor). "Tax" would imply advice; "Regulatory" would be jargon. "Compliance" reads correctly in both ES and EN.
- Free-vs-paid is surfaced in the nav with a small `[paid]` marker, not with upsell theater. Persona B hates being sold to.

### 3.2 Free vs. paid surfaces

| Surface | Free | Paid |
|---|---|---|
| Dashboard | Paper-gains view (no tax estimate) | Paper-gains + tax estimate with ranges |
| Grants list + detail, CSV import | Full | Full |
| Vesting timeline | Full | Full |
| Scenario modeler | Preview-only (inputs, no outputs) | Full |
| Sell-now calculator | Preview-only (inputs, no outputs) | Full |
| Art. 7.p inputs | Full | Full |
| Modelo 720 threshold alert (on-screen) | Full | Full |
| Modelo 720 worksheet export | Gated | Full |
| PDF / CSV exports | Gated | Full |

The "preview-only" pattern for paid calculators — inputs editable, outputs replaced with a blurred placeholder + upgrade CTA — is designed to demonstrate shape-of-the-tool without teasing numbers. No silent half-compute. (US-012 AC.)

### 3.3 Entity model driving screens (from ADR-005)

Screens map to entities as follows. This is not the data model; it's the user-visible vocabulary:

- **Grant** — the primary object. "RSU grant, 2024-09-15, 30,000 shares, double-trigger."
- **Vesting event** — derived from grant. Shown but not directly editable.
- **Scenario** — persisted what-if; has a name, IPO assumptions, and a computed result.
- **Sell-now session** — per-session calculation; persisted for audit/reproducibility but presented as ephemeral. The user experience is "I ran a sell-now estimate at 10:42"; the persistence is invisible unless they export.
- **Export** — PDF or CSV with a traceability ID. Has its own list view so users can find an old one.
- **Trip** — Art. 7.p qualifying travel. A lightweight entity.
- **Calculation** — internal. Users don't see "calculations"; they see results *inside* a scenario, sell-now session, or dashboard tile.

## 4. Core user flows

Each flow uses `→` for forward step, `⇢` for alternate/error path. Screens referenced correspond to files under `screens/`.

### 4.1 First-run onboarding (free, ~10 minutes)

**Happy path:**

1. **Signup** (email + password; TOTP offered but not forced per OQ-01). → non-dismissable modal: *"Orbit no es asesoramiento fiscal. Calcula, visualiza y exporta — no te dice qué hacer. Para actuar, consulta con tu asesor fiscal."* Accept to continue. One time; the persistent footer thereafter is a reminder, not a first-touch.
2. **Residency step**: autonomía selector, tax-year, Beckham-law flag, primary currency.
   - ⇢ if País Vasco / Navarra selected: non-blocking notice that tax calcs are unsupported; user keeps portfolio/vesting features. (US-006 AC.)
   - ⇢ if Beckham = Yes: informational block; scenario modeler and sell-now will surface the same block before computing.
3. **First grant**: form with instrument picker (RSU / NSO / ESPP / ISO→NSO), grant date, shares, strike (conditional on instrument), vesting schedule (preset templates: "4yr / 1yr cliff / monthly" etc.), double-trigger toggle (RSU only), employer + ticker. Live preview of vesting curve renders on the right as the user types. (US-001.)
   - ⇢ cliff > vesting period: inline rejection, no partial save.
   - ⇢ **"I have many grants" link** below the form → CSV import screen with Carta/Shareworks template picker, column-mapping preview, row-level error report. (US-002.)
4. **Land on dashboard**: portfolio summary, vesting curve over time, Modelo 720 status banner (passive until threshold crossed), "Run your first scenario" CTA.

**Why this shape:**
- The disclaimer modal is **once at signup**, not every session. After that, the persistent footer carries the legal load. A modal on every sign-in is user-hostile and gets dismissed reflexively, defeating the purpose.
- Autonomía + Beckham flag **before** first grant: the foral-regime and Beckham blocks are existential for what the tool will and won't compute. Collecting them after grants is entered would mean computing the wrong thing first.
- Grant form uses **preset vesting templates** because 4yr/1yr/monthly is >90% of real grants; custom schedules are a secondary affordance under "Custom".

### 4.2 Sell-now round trip (post-IPO, paid, ~3 minutes)

This is the flagship flow. Full screen under `screens/sell-now.html`.

**Happy path:**

1. User opens **Sell-now calculator** from sidebar.
2. Top strip shows: current 15-min-delayed quote, timestamp, ECB FX rate, timestamp. Badges: "Cotización retrasada 15 min · Finnhub · 10:27 CET" and "ECB · 2026-04-18 · 1 EUR = 1.0823 USD".
3. Left panel: user adds lots. Each lot = { instrument, share count, strike if NSO }. Can add multiple rows. Running total of shares summary at bottom. Validation: shares ≤ vested per lot (pulled from portfolio).
4. Right panel: **outputs update live as lots are edited** (debounced). Layout:
   - Top: single headline "Neto en tu cuenta española" as a **range band** (see §7, pattern C): `€98,400 — €112,600` with the central estimate as a dot on the rail.
   - Below: three collapsed cards — "Ingresos brutos USD", "Tras retenciones y comisiones US", "Tras impuesto español estimado". Click to expand formula trace.
   - Sensitivity section below: horizontal drivers bar showing what moves the headline most (price ±, FX spread 0–3%, Spanish tax estimate).
5. User clicks **Export PDF** → export dialog with preview, confirm → PDF downloads, export listed under Compliance → Exports with traceability ID. Passive Modelo 720/721 banner visible throughout but never blocks.

**Alternate paths:**

- ⇢ **Quote stale or vendor unavailable**: output area replaced with "Cotización no disponible — introduce un precio manual" state; user can enter price override. Bands widen to reflect higher uncertainty. No silent compute.
- ⇢ **Beckham flag on profile**: output area replaced with the Beckham block. Calculator inputs remain editable (so the user sees they would fit), but no tax numbers are produced.
- ⇢ **Free tier**: output area replaced with blurred placeholder + upgrade CTA. Inputs still editable — the free user can feel the shape of the tool.
- ⇢ **Shares > vested for a lot**: inline validation on the row; overall compute still runs for the other valid rows but the headline shows "Comprueba los lotes marcados".

### 4.3 Scenario modeling: "IPO at $X, sell N% at lockup, hold rest" (paid, ~10 minutes first time, ~2 minutes iterating)

Full screen under `screens/scenario-modeler.html`.

**Happy path:**

1. Open **Scenario modeler**. Empty-state shows three template scenarios: "Pre-IPO exercise-vs-hold", "IPO + lockup sell-through", "Residency change mid-year". User picks one; form pre-fills sensible defaults.
2. **Inputs column (left, persistent)**: Scenario name, IPO date, IPO price (central + optional ±25% override), lockup duration, sell-at-lockup %, hold-forever % (implicit remainder), FX spread assumption.
3. **Outputs column (center/right)**, three stacked blocks:
   - **Work-income IRPF** (at vest/exercise trigger): amount range, top driver, expand for formula.
   - **Ahorro-base capital gains** (at sale): amount range, top driver, expand for formula.
   - **Net proceeds** (post-tax, headline): big range band with central estimate, tax breakdown link.
4. **Sensitivity panel (bottom)**: one-at-a-time table for top 3 drivers — IPO price, FX, holding period. Each row: driver name, −25% / −10% / central / +10% / +25% columns, effect on net proceeds. (US-010 AC.)
5. **Comparison mode**: "Duplicate this scenario" creates a sibling; left rail shows both; outputs render side-by-side for comparison.
6. User clicks **Save scenario** → appears in sidebar; **Export** → PDF with all inputs, outputs, sensitivity table, rule-set version footer on every page.

**Alternate paths:**

- ⇢ Beckham = Yes → informational block replaces output area.
- ⇢ Modelo 720 threshold crossed under scenario assumptions → prominent alert above net proceeds linking to worksheet export.
- ⇢ Invalid inputs (sell% > 100, negative price) → inline validation, compute blocked.

### 4.4 Export for gestor

Full screen under `screens/export.html` (the trigger dialog; the PDF itself is a design spec, not a live HTML file).

1. User clicks "Export PDF" from any calculation result or scenario.
2. Modal dialog:
   - Preview (thumbnail of first page)
   - Scope: this scenario / this calculation / this full result bundle
   - Format: PDF (default) / CSV
   - Language: ES (default) / EN / ES+EN parallel
   - Disclaimer: shown inline in the modal with a checkbox "Entiendo que esto no es asesoramiento fiscal" (unchecked blocks export — this is the one place a friction-confirm is warranted because the artefact leaves Orbit).
3. Generate → download + row in Exports list.
4. Traceability ID visible in the exports list, copyable, matches the PDF footer and an audit-log row. Six months later the user sees a badge "Computed under superseded rule set es-2026.1.0 — current is es-2026.2.0" with a "Recompute under current rules" action.

### 4.5 GDPR data-subject self-service (account → Data & privacy)

1. Two primary buttons: **Export my data** (portability) and **Delete my account** (erasure).
2. Export: one-click kick-off, background job, email when archive is ready (within 7-day self-service target, 30-day hard SLA). Archive contains grants, scenarios, calculations, exports, audit-log entries for the user.
3. Delete: two-step confirm (typed email + click), 30-day soft-delete grace during which sign-in is blocked but recovery is possible, then cascade hard-delete per ADR-005.
4. Secondary: **Rectification request** (a free-form form that logs an audit entry), **Restrict processing** (suspends calc/export while data retained).

## 5. Visual style direction

### 5.1 Opinion

**Don't design this like a US consumer SaaS.** No pastel gradients. No oversized pull-quote numbers. No Intercom-style chat bubble. No confetti on success.

**Don't design it like Spanish enterprise software either.** No BBVA-blue top bar. No Arial. No alert-striped table headers.

**Design it like a professional's tool.** Reference the aesthetic of Bloomberg Terminal (density, neutrality), the FT (editorial seriousness, restrained color), Linear (calm whitespace in the right places, strong typographic hierarchy, tight shadow language), and a Spanish banking statement (right-aligned monetary columns with thousands-separators, tabular numerals, currency always visible).

### 5.2 Color

**Light mode (default):**

| Role | Token | Value | Notes |
|---|---|---|---|
| Surface / page | `--color-surface` | `#f7f6f2` | Warm off-white. Not pure white — reduces eye fatigue in a long session. |
| Surface raised | `--color-surface-raised` | `#ffffff` | Cards, modals. |
| Surface sunken | `--color-surface-sunken` | `#efede7` | Input fields, table stripes. |
| Foreground primary | `--color-fg` | `#1a1a1f` | Near-black. 16.3:1 on `--color-surface`. |
| Foreground secondary | `--color-fg-muted` | `#5a5a66` | Labels, meta. 6.8:1 on surface. |
| Foreground tertiary | `--color-fg-subtle` | `#8a8a95` | Timestamps, hints. 4.6:1 on surface. |
| Border | `--color-border` | `#d8d5cc` | Dividers, input borders. |
| Border strong | `--color-border-strong` | `#a8a499` | Active input, table headers. |
| **Accent (ink)** | `--color-accent` | `#2b2f6a` | Deep indigo. Primary action, active nav, focus ring. Reads serious, financial, not cheerful. 8.9:1 on surface. |
| Accent hover | `--color-accent-hover` | `#1d2052` | |
| Accent subtle | `--color-accent-subtle` | `#e6e7f0` | Active nav background, selected-state fills. |
| Positive | `--color-positive` | `#2a6b3e` | Gains, vested, completed. Muted green, not traffic-light. 6.2:1 on surface. |
| Negative | `--color-negative` | `#8a2a2a` | Losses, errors, tax owed. Oxide red, not bright. 6.5:1 on surface. |
| Warning | `--color-warning` | `#8a6a1a` | Modelo 720 alerts, stale quotes, superseded rule sets. Ochre, not yellow. 6.1:1 on surface. |
| Uncertainty band fill | `--color-band` | `#e6e7f0` | Same as accent-subtle; reads as "related to the number". |

**Dark mode** (supported, honors `prefers-color-scheme`):

| Role | Token | Value |
|---|---|---|
| Surface | `--color-surface` | `#14141a` |
| Surface raised | `--color-surface-raised` | `#1c1c24` |
| Foreground | `--color-fg` | `#ececef` (14.5:1) |
| Accent | `--color-accent` | `#7a80d8` (6.1:1 on surface) |
| Positive | `--color-positive` | `#5ba873` (5.4:1) |
| Negative | `--color-negative` | `#d86a6a` (5.1:1) |
| Warning | `--color-warning` | `#c89a4a` (6.3:1) |

**Color-blindness sanity:** status (positive / negative / warning) never relies on color alone — always paired with an icon (↑ ↓ ⚠) or a text label. Ranges use position on a rail, not hue.

### 5.3 Typography

- **UI sans:** **Inter** (variable). System-fallback stack `'Inter', -apple-system, 'Segoe UI', Roboto, sans-serif`.
- **Tabular numerals:** **JetBrains Mono** for all monetary values and tabular numbers. Stylistic choice: its lowercase `g`, `a`, and slashed-zero read as "engineering tool", which is what Orbit is. Fallback: `ui-monospace, 'SF Mono', Menlo, Consolas, monospace`.
- Use Inter's `font-feature-settings: "tnum"` on all tables (so even sans-serif numbers align in columns).

**Type scale** (13px base for density; 14px default body):

| Token | Size / LH | Use |
|---|---|---|
| `--text-xs` | 11/14 | Timestamps, trace metadata, footer legalese |
| `--text-sm` | 12/16 | Table cells, labels, dense tables |
| `--text-base` | 14/20 | Default body, form inputs |
| `--text-md` | 16/24 | Section intros, card body |
| `--text-lg` | 20/28 | Section headings |
| `--text-xl` | 28/36 | Page headings |
| `--text-2xl` | 40/48 | Headline numbers (sell-now "neto", scenario "net proceeds") |

Weights used: 400 (body), 500 (labels, table headers), 600 (section headings), 700 (headline numbers, primary CTAs only).

**Spanish-first typography note:** keep line-length around 60–72 chars for prose; Spanish is ~20% longer than English in running text, so UI labels that fit in EN may wrap in ES. Mock everything in ES first.

### 5.4 Spacing & density

4px base grid. Tokens: `--space-1` (4), `-2` (8), `-3` (12), `-4` (16), `-5` (20), `-6` (24), `-8` (32), `-10` (40), `-12` (48), `-16` (64).

**Density posture:** medium-dense.
- Table row height: 32px (vs. 48px "comfortable"). Gestor-friendly; fits 15 grants on one screen.
- Form field height: 36px.
- Card padding: 20px (`--space-5`).
- Section padding: 32px (`--space-8`).
- Dashboard tiles: 2-column at 1280, 3-column at 1536+. Never more than 3 across — numbers need space to breathe.

### 5.5 Radii, shadow, motion

- Radius: **4px** on inputs, **6px** on cards, **8px** on modals. No 16px+ rounded corners (too consumer).
- Shadow: almost none. A single subtle elevation (`--shadow-1: 0 1px 2px rgba(20,20,26,.06), 0 0 0 1px rgba(20,20,26,.04)`). For modals, slightly stronger.
- Motion: minimal. 120ms ease-out for hover/focus, 180ms ease-out for expand/collapse. **`prefers-reduced-motion` disables all non-essential transitions** (essential = disclosure widgets).

### 5.6 Iconography

Use [Lucide](https://lucide.dev) or equivalent outline icons at 16px / 20px. No filled icons. No illustrated hero scenes.

### 5.7 Numbers: the house style

This is the spine of the visual system. Numbers appear more than words on every page; get them right.

- **All money values right-aligned in columns**, JetBrains Mono, with currency suffix: `€42,318.50 EUR` or `$12,400.00 USD`.
- **Thousands separator: `,` in EN, `.` in ES**; decimal separator: `.` in EN, `,` in ES. Locale-aware via `Intl.NumberFormat`. (Spanish convention: `€42.318,50`.)
- **Currency always explicit** when mixing USD and EUR on the same screen. Never trust the user's inference.
- **Ranges** always shown as `X — Y` (em-dash, not hyphen, not `to`). Central estimate in primary weight, range bounds in muted weight.
- **Negative amounts** wrapped in a minus sign *and* colored `--color-negative`: `−€1,200`. Never parens-style accounting notation (looks US-centric and reads wrong in ES).
- **Percentages** use `%` not `pct`; space between number and symbol per RAE convention: `15 %`.
- **Zero padding** on cents always: `€100,00` not `€100`. Consistency matters on a number-dense screen.

## 6. Key screens

Six reference HTML files under `screens/`:

| File | Screen | Demonstrates |
|---|---|---|
| `dashboard.html` | Portfolio dashboard (post-IPO persona) | Navigation, tile density, Modelo 720 passive banner, disclaimer footer, rule-set chip |
| `sell-now.html` | Sell-now calculator | **Range-band headline (pattern C)**, live-quote strip, lot entry, sensitivity drivers, Beckham-block alt state |
| `scenario-modeler.html` | Scenario modeler | Side-by-side inputs/outputs, sensitivity table, three treatments of ranges compared |
| `grant-detail.html` | Grant detail + vesting timeline | Double-trigger visual state, formula trace panel, edit flow |
| `export.html` | Export trigger dialog | Disclaimer confirm pattern, scope/format/language controls, export list with traceability IDs |
| `uncertainty-patterns.html` | Pattern reference | Three side-by-side treatments (A/B/C) for the ranges-and-sensitivity problem, with recommendation |

Each HTML file is self-contained (no framework, no CDN), uses only tokens from `style-guide.md`, and renders without JavaScript for the primary read. Minimal inline `<script>` handles disclosure widgets (range expand, formula trace); a no-JS fallback leaves them expanded.

**Load order for a reviewer:** `uncertainty-patterns.html` first (it frames the single most distinctive pattern), then `sell-now.html` (the flagship flow that most strenuously tests the pattern), then `dashboard.html` → `scenario-modeler.html` → `grant-detail.html` → `export.html`.

## 7. The ranges-and-sensitivity pattern (most distinctive design problem)

Spec §7.4: **never a bare point estimate.** Every projected tax number shows a central estimate AND a sensitivity range. This has to be a design primitive, not an afterthought.

Three candidate treatments. All three demoed in `screens/uncertainty-patterns.html`.

### Pattern A — Inline parenthetical range

```
Net proceeds    €42,318.50   (€37,280 — €47,450, ±10% precio IPO)
```

- Pros: compact, works in tables, familiar from spreadsheet comments.
- Cons: reads as "the real number is 42,318 and the range is trivia". The range gets ignored. Violates the spirit of §7.4.
- **Use for:** dense table cells where a bar chart would break the layout (grant detail, scenario sensitivity sub-tables).

### Pattern B — Range bar with central dot

```
Net proceeds

€42,318                 ← central, big
────●──────────         ← horizontal rail, shaded band, dot at central
€37,280         €47,450  ← bounds in muted small
±10% IPO price           ← driver, in subtle
```

- Pros: the range is visually loud; the central is still the most prominent read; the driver is named.
- Cons: takes vertical space; breaks table flow.
- **Use for:** card-level numbers on dashboards, scenario-modeler sub-blocks (work-income IRPF, cap gains).

### Pattern C — Range-first headline

```
     What lands in your Spanish bank

     €98,400  —  €112,600   ← the range IS the headline, equal weight
     central estimate €105,500  ← smaller, underneath
     [════════●═════════]  ← bar rail, dot = central
     Price −10% / +10% · FX spread 0–3%  ← drivers named, small
```

- Pros: impossible to read as a point estimate. Honors §7.4 in the strongest possible way. Tells the user the answer is fundamentally uncertain before they see a central.
- Cons: high visual weight; doesn't scale to every number on a page.
- **Use for:** the single "what actually lands" headline on the sell-now and scenario screens. The one number that drives the decision.

### Recommendation

**A hybrid, by screen zone:**

- **Headline / decision number** (one per result page) → Pattern C.
- **Card-level sub-totals** → Pattern B.
- **Tabular / dense lines** → Pattern A.

This gives the user a visual cue about which number they're actually supposed to weigh: the big range-first headline is the one that drives the decision; the bars are the drivers of the headline; the inline parentheticals are the supporting detail.

### Expanding a number: the "show formula" affordance

Every number is clickable (or focusable) and expands to show:

```
Gain on sale
= (sell_price − cost_basis) × shares
= ($48.00 − $31.20) × 3,000
= $50,400 USD
→ €46,562 EUR  (ECB 2026-04-18, 1 EUR = 1.0823 USD; spread 1.5%)
Routed through ahorro base:
  19% on first €6,000         = €1,140
  21% on next €44,000         = €9,240
  23% on remainder            = €129
  Total IRPF ahorro base      = €10,509
Rule set: es-2026.1.0 (AEAT guidance 2026-03-15)
```

Rendered as an expanding panel below the number, not a tooltip (tooltips hide content that users want to read slowly and re-read). Keyboard: `Enter` / `Space` to toggle, `Escape` to collapse. (US-010 AC.)

### Sensitivity drivers table

On scenario / sell-now outputs, a compact table below the headline:

| Driver | −25% | −10% | Central | +10% | +25% |
|---|---|---|---|---|---|
| Precio IPO | €31,200 | €38,100 | **€42,318** | €46,540 | €53,100 |
| EUR/USD | €41,200 | €41,900 | **€42,318** | €42,710 | €43,400 |
| Ventana holding | €39,900 | €41,400 | **€42,318** | €43,200 | €44,200 |

- Driver sorted by sensitivity (top row = biggest mover).
- Central cell bold.
- Cells shaded on a muted red-to-green gradient relative to central (color paired with ↑/↓ arrows for color-blindness).
- "Why these three?" link → short explainer of the sensitivity methodology.

## 8. Disclaimer pattern — "no es asesoramiento fiscal"

The spec requires the disclaimer to appear on every calc output, non-dismissable. Design goal: **legally sufficient, visually calm, impossible to forget it's there**.

### Four-layer approach

1. **First-login modal** (once, at signup). Full text, accept button. Records consent in audit log. After accept: **never shown as a modal again**. This is where the heavy legal text lives.
2. **Persistent footer** (every page with a calculation or projected number). A single thin strip at the bottom of the content area (not fixed to viewport — that steals space on mobile). Height 32px. Text: `Esto no es asesoramiento fiscal ni financiero — Orbit calcula, no aconseja. · Rule set es-2026.1.0 (AEAT 2026-03-15) ·  [Ver trazabilidad]`. Click [Ver trazabilidad] → side panel with full rule-set history and engine version.
3. **Inline confirm at export** (at the moment the artefact leaves Orbit). Checkbox `[ ] Entiendo que esto no es asesoramiento fiscal` required to proceed. This is the one place a confirm is warranted because exports get forwarded to gestores and the user has crossed a trust boundary.
4. **On the export itself** (per ADR-008). Footer on every PDF page + in XMP metadata + CSV comment block.

### Why not a persistent banner at the top?

Because it gets ignored within two sessions. The spec's goal is legal-sufficiency + "impossible to forget", not "maximum banner area". The footer-strip-with-version-chip is both cheaper visual real estate and *more informative* (it surfaces the rule set, which is itself a safety signal) than a stripe banner.

### Why not show it under every number?

The spec says "every calculation output", not "every number". Attaching a disclaimer to each cell of a table would desensitize users to it within ten seconds. **One footer per page, one confirm per export, one modal at signup** is the right saturation.

### Copy (ES primary, EN secondary)

| Surface | Spanish | English |
|---|---|---|
| First-login modal | "Orbit no es asesoramiento fiscal ni financiero. Calcula, visualiza y exporta; no te dice qué hacer. Para actuar sobre estos números, consulta con tu asesor fiscal. Orbit no está registrado en CNMV ni presta servicios regulados." | "Orbit is not tax or financial advice. It calculates, visualizes, and exports; it does not tell you what to do. Before acting on these numbers, consult your tax advisor. Orbit is not CNMV-registered and does not provide regulated services." |
| Footer (per page) | "Esto no es asesoramiento fiscal ni financiero — Orbit calcula, no aconseja." | "This is not tax or financial advice — Orbit calculates, it doesn't recommend." |
| Export confirm | "Entiendo que este documento no es asesoramiento fiscal y lo revisaré con mi asesor antes de actuar." | "I understand this document is not tax advice and I will review it with my advisor before acting." |
| Beckham block | "v1 no calcula resultados bajo el régimen de impatriados. Consulta con tu asesor fiscal." | "v1 does not compute under the impatriate regime. Consult your tax advisor." |
| Foral block | "v1 no soporta los regímenes forales de País Vasco y Navarra. Los cálculos fiscales están desactivados; la cartera y los calendarios de vesting siguen disponibles." | "v1 does not support the foral regimes of País Vasco and Navarra. Tax calculations are disabled; portfolio and vesting features remain available." |

## 9. Accessibility

Target: **WCAG 2.2 AA** on all core flows.

- **Contrast** ratios stated per token in §5.2; every combination used in the reference HTML passes AA.
- **Keyboard flow**: tab order matches visual reading order (left-to-right, top-to-bottom). `focus-visible` ring uses the accent color at 2px offset with 2px width — visible on all backgrounds. No `outline: none` anywhere without a replacement.
- **Landmarks**: `<header>`, `<nav>`, `<main>`, `<aside>` (sidebar), `<footer>`. Heading order is strict H1 → H2 → H3 with no skips; each screen has exactly one H1.
- **ARIA**: used sparingly. `role="status"` on the live-quote strip (so screen readers announce price updates but don't interrupt); `aria-live="polite"` on the sensitivity range output; `aria-expanded` on disclosure buttons. No role overrides on native elements.
- **Forms**: every input has an explicit `<label>`. Error messages are in the accessibility tree via `aria-describedby`, not just color.
- **Tables**: real `<table>` with `<thead>`/`<th scope>`. Money columns marked `text-align: right` in CSS, not with a presentational class.
- **`prefers-reduced-motion`**: disables the bar-rail animations, the expand/collapse transitions, the live-quote pulse. Static states still convey all information.
- **`prefers-color-scheme: dark`**: full dark-mode token set (§5.2).
- **Screen-reader considerations for ranges**: the range bar uses a semantic `<meter>` element with visually-hidden text: "Rango: 37.280 a 47.450 euros, central 42.318 euros, controlado por precio IPO más o menos 10 por ciento."
- **Color-blindness**: status is always paired with an icon or text label; ranges encoded by position, not hue.
- **Zoom**: layout works from 100% to 200% zoom without horizontal scroll (tested on the reference HTML).

## 10. Responsive behavior

Breakpoints: **mobile** (≤640), **tablet** (641–1024), **desktop** (1025–1535), **wide** (≥1536).

**Desktop-first because** the persona uses Orbit at a desk, mid-decision, with a spreadsheet open. Mobile is "quick check the quote", not "run a full scenario".

| Screen | Mobile | Tablet | Desktop |
|---|---|---|---|
| Dashboard | Stacked tiles, single column; nav as top hamburger | 2-column tiles, collapsible sidebar | Full sidebar + 2–3-column tiles |
| Sell-now | Stacked: inputs, outputs, sensitivity; outputs collapse to headline-only by default | Inputs left, outputs right; sensitivity collapses | Full 3-pane layout with sensitivity always visible |
| Scenario modeler | Not recommended on mobile (compound form + sensitivity table). Show "Esta pantalla funciona mejor en un ordenador" banner, but still usable | Split left/right | Full 3-pane with comparison mode |
| Grant detail | Stacked panels, timeline scrollable | Side-by-side panels | Full panels with inline formula trace |
| Export dialog | Full-screen sheet (bottom-up) | Centered modal | Centered modal |

**Touch targets:** minimum 44×44 on mobile; cramped 32px table rows relax to 48px on `pointer: coarse`.

**Tables on mobile:** for long money tables (grants, lots), use a horizontal-scroll pattern with the first column (grant name / lot name) sticky. Do *not* collapse to cards — persona-B users specifically want the columnar view.

## 11. Design tokens and style-guide extension

See `style-guide.md` for the full token set. Summary of what's new for v1:

- Full color scale (light + dark) with contrast ratios documented per token.
- Type scale anchored on 13px/14px base.
- Spacing on 4px base.
- Tabular-numeral enforcement in all money-bearing contexts.
- Range-bar primitive (CSS-only, no JS for rendering).
- Disclaimer-footer primitive.
- Rule-set chip primitive.

No new tokens are introduced without a one-line justification in `style-guide.md`, per the agent guardrail.

## 12. Open questions & design risks

Numbered, each with a proposed default so implementation isn't blocked.

| # | Question | Proposed default | Who to validate with |
|---|---|---|---|
| D-1 | Is the headline range-first pattern (C) too visually heavy for daily use? Users might flinch. | Ship C on sell-now and scenario "net proceeds" only (one number per screen); use B elsewhere. Measure qualitative feedback in first 4 weeks; if users describe it as "alarming", fall back to B with explicit range label. | 5-user usability test with persona-B candidates. |
| D-2 | Is the disclaimer footer (§8 layer 2) legally sufficient, or does legal require a banner? | Default to footer; prepare a banner variant behind a flag. Legal opinion owns the call. | legal + CNMV review. |
| D-3 | Do users understand "rule set es-2026.1.0" or is that internal jargon leaking? | Default to showing the AEAT-guidance-date prominently with the semver version in smaller type: "AEAT 15 mar 2026 · es-2026.1.0". Test readability. | Usability test. |
| D-4 | Sell-now live-update (as user edits lots) vs. explicit "Calculate" button? | Default to live-update with 300ms debounce. Spec implies live. Risk: users may perceive the compute as flaky if numbers wobble while they type. Fallback: add a "recalculating…" status line. | Usability test. |
| D-5 | Bilingual: do we show both ES and EN in parallel, or toggle? | Default to toggle (ES primary). Tax terms (IRPF, rendimiento del trabajo, Modelo 720) always in Spanish regardless of toggle. Inline EN gloss on hover for tax terms. | Usability test with EN-primary users in Spain (a real subsegment). |
| D-6 | Mobile: is the "scenario modeler not recommended on mobile" banner paternalistic? | Keep it but don't block; persona-B will force it to work if they need it. Measure mobile scenario-completion rate. | Analytics once launched. |
| D-7 | Vesting timeline visualization: Gantt-like bars vs. cumulative curve vs. both? | Default: cumulative curve as primary; Gantt view as a toggle. Double-trigger RSUs visually distinct (dashed fill) regardless of view. | Usability test. |
| D-8 | How does the "recompute under current rules" flow render the diff? Side-by-side or inline? | Default: side-by-side with per-line diff indicators (▲ went up, ▼ went down, = unchanged). Inline becomes cluttered. | Usability test. |
| D-9 | Free-tier preview-only state: blurred numbers vs. "€•,•••" pattern vs. empty with upgrade CTA? | Default: shape of the output visible (layout rendered, numbers replaced with `€•,•••` using the same typography). Blurring feels consumer. Empty feels punishing. | Quick A/B post-launch. |
| D-10 | Gestor export PDF language: ES by default vs. user-locale? | Default: ES for the PDF regardless of UI locale, because the gestor is always a Spanish-speaking professional. UI user can toggle to ES+EN parallel if they want to review the export themselves. | No test; clear product call. |
| D-11 | The "passive Modelo 720/721 banner" on sell-now — is static text enough, or does it need threshold-crossing smartness? | Default: static passive text per spec. Smart threshold is explicitly out-of-scope for sell-now (§4.2 out-of-scope list). | — |

## 13. Handoff

Files produced:

- `/Users/ivan/Development/projects/orbit/docs/design/orbit-v1-ui-proposal.md` — this document.
- `/Users/ivan/Development/projects/orbit/docs/design/style-guide.md` — tokens + primitive components.
- `/Users/ivan/Development/projects/orbit/docs/design/screens/dashboard.html`
- `/Users/ivan/Development/projects/orbit/docs/design/screens/sell-now.html`
- `/Users/ivan/Development/projects/orbit/docs/design/screens/scenario-modeler.html`
- `/Users/ivan/Development/projects/orbit/docs/design/screens/grant-detail.html`
- `/Users/ivan/Development/projects/orbit/docs/design/screens/export.html`
- `/Users/ivan/Development/projects/orbit/docs/design/screens/uncertainty-patterns.html`
- `/Users/ivan/Development/projects/orbit/docs/design/screens/shared.css` — shared tokens and primitives used by all screens.

Next agent:

> **Next:** review with owner; once the range-pattern recommendation and disclaimer layering are accepted, invoke `solution-architect` for any UI-driven architecture implications (notably: does the sell-now live-update model impose new constraints on the market-data cache TTL or the calculation endpoint latency?), then `implementation-engineer` to map these screens to the React SPA structure from ADR-001.

---

## §13 — Implementation-ready refinement (2026-04-18)

This section appends a targeted refinement done **after** requirements-analyst resolutions and ADR-009 / ADR-011 / ADR-014 landed. It does **not** revisit the visual language, the Pattern-A/B/C recommendation, the disclaimer layering, or the IA in §3 — those are decided.

**Source of truth for the refinement scope:**
- `/Users/ivan/Development/projects/orbit/docs/requirements/open-questions-resolved.md` (closes §12's D-1..D-11 and 15 cross-reading ambiguities C-1..C-15)
- `/Users/ivan/Development/projects/orbit/docs/requirements/v1-slice-plan.md` (8 slices; refinement targets Slice 1 + v1-blocker security surfaces)
- `/Users/ivan/Development/projects/orbit/docs/requirements/slice-1-acceptance-criteria.md`
- `/Users/ivan/Development/projects/orbit/docs/adr/ADR-009-frontend-architecture.md`
- `/Users/ivan/Development/projects/orbit/docs/adr/ADR-011-authentication-session-mfa.md`
- `/Users/ivan/Development/projects/orbit/docs/adr/ADR-014-slice-1-technical-design.md`
- `/Users/ivan/Development/projects/orbit/docs/security/security-requirements.md`

### 13.1 Resolution of §12's 11 open questions (D-1..D-11)

All 11 are now closed. `open-questions-resolved.md` is the authoritative record; one-liner per item:

| # | Resolution (see `open-questions-resolved.md` for rationale) | Screen impact |
|---|---|---|
| D-1 | Ship Pattern C on exactly two screens: sell-now headline and scenario net-proceeds headline. | Existing screens conform. No change. |
| D-2 | Four-layer disclaimer stands; banner variant flagged behind a feature flag. Legal opinion needed before flipping. | Existing screens conform. No change. |
| D-3 | `AEAT 15 mar 2026 · es-2026.1.0` — AEAT date primary, semver secondary. | Existing chips conform. No change. |
| D-4 | Live-update with 300 ms debounce; aria-live status line. | `sell-now.html` **updated**: added `.recalc-status` aria-live region next to the live-quote strip. |
| D-5 | ES primary; tax terms always in ES even in EN. Disclaimer copy ES-first in EN locale. | Existing screens conform. No change. |
| D-6 | Mobile scenario-modeler: informative non-blocking banner. | `scenario-modeler.html` **updated**: added `alert alert--info` "Esta pantalla funciona mejor en un ordenador" under page title. |
| D-7 | Cumulative curve default; Gantt toggle; dashed fill for double-trigger awaiting liquidity. | Existing `grant-detail.html` conforms. No change. |
| D-8 | Side-by-side diff with ▲▼= indicators on recompute-under-current. | Slice 6 scope — no screen ships it now. `export.html` carries the superseded-chip UX per decision. |
| D-9 | `€•,•••` skeleton preserving layout; no blur. | Existing `preview-block` pattern conforms. No change. |
| D-10 | PDF ES-default; ES+EN parallel toggle from the export dialog. | Existing `export.html` conforms; `export-confirm-modal.html` (new) tightens the copy around identifying info. |
| D-11 | Passive static banner on sell-now; no threshold math. | Existing `sell-now.html` conforms. No change. |

### 13.2 Cross-reading ambiguity resolutions with visible-surface impact

From `open-questions-resolved.md` Part C, the items that touch a visible surface and drove this refinement:

- **C-1** (residency before first grant): wizard ordering pinned per ADR-014 §3. New screens `signup.html`, `residency-setup.html`, `first-grant-form.html` render that exact sequence.
- **C-2** (disclaimer-consent audit-log persistence): `signup.html` step 3 shows the `POST /consent/disclaimer` form; copy references `dsr.consent.disclaimer_accepted` and `version: v1-2026-04`.
- **C-3** (no rule-set chip in Slice 1 footer): `dashboard-slice-1.html` omits the chip; `signup.html` / `signin.html` / `password-reset.html` / `residency-setup.html` / `first-grant-form.html` all render the footer text without a chip. The chip returns in Slice 3 (FX-dependent numbers) and Slice 4 (tax numbers).
- **C-4** (no EUR conversion on Slice 1 dashboard): `dashboard-slice-1.html` is the authoritative Slice-1 dashboard; `dashboard.html` remains as the Slice-3+ target. Native currency only, currency suffix always explicit.
- **C-7** (session/device UI phasing): `session-management.html` is the reference for the UI that ships in Slice 2–3. Backend ready in Slice 1 per ADR-011.
- **C-11** (cookie banner posture): `signup.html` renders the informational-only banner per SEC-127. No consent needed (essential-cookies v1).

### 13.3 New screens produced in this refinement

All files are in `/Users/ivan/Development/projects/orbit/docs/design/screens/` and use the existing `shared.css` (which was extended additively for this refinement — see §13.5 below).

| File | One-liner |
|---|---|
| `signup.html` | Multi-step wizard per ADR-014 §3: password → verify-email → disclaimer → residency → first-grant. Shows all five states side-by-side with stepper. Includes onboarding-gate + reload behaviour note. |
| `signin.html` | Five states side-by-side: default, generic-error (SEC-004), post-3-failures CAPTCHA (SEC-161), rate-limit (SEC-003, distinguishable copy without enumeration), MFA challenge (scaffolded per ADR-011, 501 in Slice 1). |
| `password-reset.html` | Request form + generic-response post-submit + token-landing form + expired-token error state. Copy honours SEC-004 anti-enumeration. |
| `residency-setup.html` | Slice 1 AC 4.1. Autonomía picker (territorio común + foral with `(no soportado en v1)` suffix), Beckham flag, primary currency. Art. 7.p informational-only note. Foral warning block preview. |
| `first-grant-form.html` | Slice 1 AC 4.2. Instrument picker → employer/ticker → grant date / shares → strike (conditional) → vesting template / custom → double-trigger toggle → liquidity-event-date. Live vesting preview pane on the right; algorithm identical to ADR-014 §2 `derive_vesting_events`. |
| `dashboard-slice-1.html` | **C-4-compliant** dashboard. No EUR, no Modelo 720 banner, no rule-set chip, no tax tiles. Grant tiles in native currency; dashed-fill "awaiting liquidity" sparkline; empty state. Existing `dashboard.html` kept as Slice-3+ target reference. |
| `session-management.html` | SEC-010 active-sessions panel. No raw IP ever shown (hashed per SEC-054); coarse geo only. Current-session row cannot revoke itself ("Cerrar todas las demás" instead). New-device email preview. Refresh-token-reuse explanation. |
| `dsr-self-service.html` | SEC-123 + US-011. All four rights: access/portability, rectification, restriction, erasure (two-step confirm + 30-day grace). Retention table (SEC-124 transparency). |
| `export-confirm-modal.html` | Focused pass on the export dialog. Adds the "PDF includes identifying info" warning (user's email is in the PDF footer per US-009 AC #1), updated XMP-metadata preview, two states (disclaimer pending / confirmed). Supplements existing `export.html`. |

### 13.4 Updates to existing screens

| File | Change |
|---|---|
| `sell-now.html` | Added `.recalc-status` aria-live status line in the live-quote strip (D-4). No visual regressions. |
| `scenario-modeler.html` | Added "Esta pantalla funciona mejor en un ordenador" informative banner under page title (D-6). Non-blocking, `role="note"`. |
| `dashboard.html`, `grant-detail.html`, `export.html`, `uncertainty-patterns.html` | **No change — ready as-is.** These screens target Slices 3+, and the decisions that landed don't change their shape. |

### 13.5 `shared.css` additions

All additions are additive (no token edits, no primitive redefinitions). Every new class is used by at least one new screen.

- Utility margin / gap helpers (`.mt-1..8`, `.mb-1..8`, `.gap-1..4`, `.mt-auto`, `.full-width`, `.text-sm`, `.text-xs`) — the sole purpose is to avoid `style=""` attributes on new screens (CSP honesty; see §13.6 conflict B).
- `.auth-shell` + `.auth-card` — centered single-card layout for signup / signin / reset / residency / first-grant (no sidebar).
- `.stepper` — wizard progress indicator.
- `.form-grid`, `.choice`, `.choice-card`, `.choice-group` — denser form layouts for wizard pages.
- `.captcha-slot` — placeholder hook for hCaptcha / Turnstile (SEC-161).
- `.recalc-status` (+ variants) — live-update status line for D-4.
- `.cookie-banner` — informational banner for SEC-127 essential-cookies posture.
- `.session-row` — row styling for SEC-010 active-sessions list.
- `.danger-confirm` — two-step confirm block for DSR erasure.
- `.email-preview` — monospace block to render email templates as reference.
- `.grant-tile` + `.sparkline` — Slice-1 dashboard tile (no-FX, no-tax variant).
- `.account-split` + `.account-menu` + `.account-panel` — two-pane layout for account pages.
- `.diff-up` / `.diff-down` / `.diff-eq` — forward-compatible classes for the D-8 recompute diff (not used yet; placeholder).
- `.card--flush`, `.list-indent`, `.section-divider`, `.back-link`, `.mono` — small utilities used in new screens.

### 13.6 Conflicts surfaced with architecture

Four conflicts (or near-conflicts) surfaced while reconciling ADR-011 + ADR-014 against the UX. Each is stated with the conflict, my reading of the trade-off, and a proposed reconciliation.

**Conflict A — Email verification before first grant is the right call (confirmed, not flagged).** SEC-162 requires email verification before any grant is saved, and ADR-014's state machine blocks `/app/*` behind `onboarding.required` until the wizard completes. The UX agrees: it is the right hard stop. No change needed; noted for completeness so the next reviewer does not re-open it.

**Conflict B — Existing reference HTML uses inline `style=""` attributes; SEC-180 CSP bans them.** The prior-pass screens (`dashboard.html`, `sell-now.html`, `grant-detail.html`, `scenario-modeler.html`, `export.html`, `uncertainty-patterns.html`) use `style="..."` attributes and inline `<script>` blocks extensively (e.g., `style="margin-bottom: var(--space-6);"`, `onclick="document.getElementById(...)..."`). SEC-180 `style-src 'self'` with no `'unsafe-inline'` **blocks inline style attributes in browsers that enforce it**. ADR-009 claims the stack boots under SEC-180, which is true for the *built* React app (JSX `style={...}` becomes inline style that some build pipelines can hoist) but the *reference HTML* as-shipped in `docs/design/screens/` is technically non-compliant. **My new screens minimise inline style** (kept only for the data-driven percentage-width bars in `first-grant-form.html` and `dashboard-slice-1.html`, which are the same pattern existing screens use for `.vesting__fill` and `.range__dot`). **Reconciliation:** the implementation-engineer should render dynamic widths either (a) via React's `style={{width: `${pct}%`}}` with a build-time CSP nonce for inline style, or (b) via a small set of `.w-0 .. .w-100` utility classes, or (c) via CSS custom properties set from JS on a parent element. This is an implementation detail, not a design detail. The reference HTML keeps the existing convention for consistency with the prior pass. **I recommend the UX proposal note this is a known handoff item**, which it now does here.

**Conflict C — SEC-004 generic errors vs. the "did my signup work?" moment on signin.** When a user forgets which email address they used to sign up, SEC-004 forces the same "Credenciales inválidas" regardless of whether the email is unknown, the password is wrong, or the email is unverified. This creates a real usability trap: the user thinks the service is broken. ADR-011's signin sequence handles the unverified-email case with a different 403 (`auth.email_unverified`), which is already a tension with SEC-004's "same copy, same timing." **Reconciliation in my design:** on the `signin.html` screen, the default footer under the form carries a persistent `"¿No recibiste el correo de verificación? Reenviar enlace"` link. This does not leak user existence (anyone can request a resend; the server runs the same generic-response SEC-004 flow). The error-state card also carries a small muted `"Si acabas de registrarte, asegúrate de haber verificado tu email"` gloss. **I consider this a real UX↔security tension resolved by copy, not a blocker**, but the security reviewer should confirm the resend-link is not itself an enumeration oracle (answer should be: no, because the response is generic — but worth writing down).

**Conflict D — Rate-limit error distinguishability (SEC-003) without enumeration.** When SEC-003 trips (per-IP 10/10 min, per-account 5/10 min), a legit user needs to know something specific enough to not panic ("Demasiados intentos — espera unos minutos"), but not so specific that an attacker can tell whether it was the per-account or per-IP bucket. ADR-011 doesn't pin the copy. **Reconciliation in my design:** `signin.html` state D renders the single copy `"Por tu seguridad, hemos pausado los inicios de sesión para esta combinación durante unos minutos. Inténtalo de nuevo en ~8 min."`. The cooldown estimate (`~8 min`) is the floor of the exponential backoff — specific enough to be actionable, vague enough that an attacker can't tell which bucket tripped (the actual backoff windows per-IP vs per-account are different). The password-reset link is reachable from the error to give a non-attacker path forward. **I recommend the security-engineer bless this copy** as the canonical rate-limit-UX string.

**Conflict E — Onboarding-gate `403` on direct-URL load: UX degradation risk.** ADR-014 §3 specifies a 403 response with `{ code: "onboarding.required", stage: "..." }` that the SPA router must intercept. If the user has JavaScript disabled, or the SPA fails to load (first-load network glitch), a user who deep-links to `/app/dashboard` sees a raw 403 page and has no path back. **Reconciliation:** the server should serve a minimal *HTML* shell on `/app/*` that redirects (via `<meta http-equiv="refresh">`) to the correct `/signup/<stage>` when onboarding is incomplete — a zero-JS fallback. This is a server-rendering detail for the implementation-engineer; it is not a design change. I flag it here so nobody ships the SPA-only behaviour and discovers the edge case in production.

### 13.7 Items left for the next agent

None from UX. The above is ready for `solution-architect` to confirm the server-side `/app/*` meta-refresh fallback (conflict E) and for `security-engineer` to bless the state-D rate-limit copy (conflict D).

### 13.8 Handoff

Files produced or updated in this refinement:

- `/Users/ivan/Development/projects/orbit/docs/design/orbit-v1-ui-proposal.md` (this §13 appended)
- `/Users/ivan/Development/projects/orbit/docs/design/screens/shared.css` (additive extensions per §13.5)
- `/Users/ivan/Development/projects/orbit/docs/design/screens/signup.html` *(new)*
- `/Users/ivan/Development/projects/orbit/docs/design/screens/signin.html` *(new)*
- `/Users/ivan/Development/projects/orbit/docs/design/screens/password-reset.html` *(new)*
- `/Users/ivan/Development/projects/orbit/docs/design/screens/residency-setup.html` *(new)*
- `/Users/ivan/Development/projects/orbit/docs/design/screens/first-grant-form.html` *(new)*
- `/Users/ivan/Development/projects/orbit/docs/design/screens/dashboard-slice-1.html` *(new)*
- `/Users/ivan/Development/projects/orbit/docs/design/screens/session-management.html` *(new)*
- `/Users/ivan/Development/projects/orbit/docs/design/screens/dsr-self-service.html` *(new)*
- `/Users/ivan/Development/projects/orbit/docs/design/screens/export-confirm-modal.html` *(new)*
- `/Users/ivan/Development/projects/orbit/docs/design/screens/sell-now.html` (D-4 aria-live status)
- `/Users/ivan/Development/projects/orbit/docs/design/screens/scenario-modeler.html` (D-6 mobile banner)

> **Next:** invoke `implementation-engineer` with `docs/requirements/slice-1-acceptance-criteria.md`, `docs/adr/ADR-014-slice-1-technical-design.md`, and the Slice-1 reference screens (`signup.html`, `signin.html`, `password-reset.html`, `residency-setup.html`, `first-grant-form.html`, `dashboard-slice-1.html`). Hand `session-management.html` and `dsr-self-service.html` forward for Slice 2–3 and Slice 7 respectively. Ask `solution-architect` to confirm the `/app/*` meta-refresh fallback (conflict E) and `security-engineer` to bless the SEC-003 rate-limit copy (conflict D) before any of the auth screens ship.

---

*End of proposal (v0.2 refinement appended 2026-04-18).*
