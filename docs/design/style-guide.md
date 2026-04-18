# Orbit style guide

Tokens and primitive components. Every token is referenced from the HTML under `screens/`. Extend this file when adding a new token; include a one-line justification.

## 1. Color

### Light (default)

| Token | Value | Contrast vs `--color-surface` | Use |
|---|---|---|---|
| `--color-surface` | `#f7f6f2` | — | Page background; warm off-white to reduce eye fatigue in long sessions |
| `--color-surface-raised` | `#ffffff` | — | Cards, modals, popovers |
| `--color-surface-sunken` | `#efede7` | — | Input backgrounds, zebra-striped tables |
| `--color-fg` | `#1a1a1f` | 16.3:1 | Primary body text |
| `--color-fg-muted` | `#5a5a66` | 6.8:1 | Labels, column headers, meta |
| `--color-fg-subtle` | `#8a8a95` | 4.6:1 | Timestamps, hints, decoration |
| `--color-border` | `#d8d5cc` | — | Field borders, dividers |
| `--color-border-strong` | `#a8a499` | — | Active fields, table header underline |
| `--color-accent` | `#2b2f6a` | 8.9:1 | Primary CTA, active nav, focus ring (deep indigo — serious, financial, non-consumer) |
| `--color-accent-hover` | `#1d2052` | 10.4:1 | Hover state for accent |
| `--color-accent-subtle` | `#e6e7f0` | — | Selected/active-nav fill, uncertainty-band fill |
| `--color-positive` | `#2a6b3e` | 6.2:1 | Gains, vested, success (muted green) |
| `--color-negative` | `#8a2a2a` | 6.5:1 | Losses, errors, tax owed (oxide red) |
| `--color-warning` | `#8a6a1a` | 6.1:1 | Modelo 720 alerts, stale data, superseded rule sets (ochre) |
| `--color-band` | `#e6e7f0` | — | Range-bar fill (= accent-subtle) |

### Dark (`prefers-color-scheme: dark`)

| Token | Value | Contrast vs `--color-surface` |
|---|---|---|
| `--color-surface` | `#14141a` | — |
| `--color-surface-raised` | `#1c1c24` | — |
| `--color-surface-sunken` | `#101016` | — |
| `--color-fg` | `#ececef` | 14.5:1 |
| `--color-fg-muted` | `#a0a0ac` | 7.4:1 |
| `--color-fg-subtle` | `#70707c` | 4.7:1 |
| `--color-border` | `#2e2e38` | — |
| `--color-border-strong` | `#50505c` | — |
| `--color-accent` | `#7a80d8` | 6.1:1 |
| `--color-accent-hover` | `#8f95e8` | 7.2:1 |
| `--color-accent-subtle` | `#2a2e4a` | — |
| `--color-positive` | `#5ba873` | 5.4:1 |
| `--color-negative` | `#d86a6a` | 5.1:1 |
| `--color-warning` | `#c89a4a` | 6.3:1 |
| `--color-band` | `#2a2e4a` | — |

## 2. Type scale

Base: 14px body, 13px tables. Font-feature `tnum` always on for money.

| Token | Size / LH | Weight | Use |
|---|---|---|---|
| `--text-xs` | 11/14 | 400 | Timestamps, traceability metadata, footer legalese |
| `--text-sm` | 12/16 | 400 / 500 | Table cells; 500 for column headers |
| `--text-base` | 14/20 | 400 | Default body, form inputs |
| `--text-md` | 16/24 | 400 / 500 | Section intros, card body |
| `--text-lg` | 20/28 | 600 | Section headings (H3) |
| `--text-xl` | 28/36 | 600 | Page headings (H1) |
| `--text-2xl` | 40/48 | 700 | Headline decision numbers |

Font stacks:

```css
--font-sans: 'Inter', -apple-system, 'Segoe UI', Roboto, sans-serif;
--font-mono: 'JetBrains Mono', ui-monospace, 'SF Mono', Menlo, Consolas, monospace;
```

**Always apply** on any container with money or tabular numbers:

```css
font-variant-numeric: tabular-nums;
font-feature-settings: "tnum";
```

## 3. Spacing (4px base)

```
--space-1: 4px   --space-2: 8px   --space-3: 12px   --space-4: 16px
--space-5: 20px  --space-6: 24px  --space-8: 32px   --space-10: 40px
--space-12: 48px --space-16: 64px
```

Container heights: form field 36px, table row 32px, button 36px (compact) / 40px (default).

## 4. Radii, shadow, motion

```
--radius-sm: 4px    (inputs, pills)
--radius-md: 6px    (cards, buttons)
--radius-lg: 8px    (modals)

--shadow-1: 0 1px 2px rgba(20,20,26,.06), 0 0 0 1px rgba(20,20,26,.04);
--shadow-2: 0 4px 12px rgba(20,20,26,.10), 0 0 0 1px rgba(20,20,26,.06);  /* modals */

--motion-fast: 120ms;
--motion-med:  180ms;
--ease: cubic-bezier(.2,.7,.2,1);
```

Reduced motion:

```css
@media (prefers-reduced-motion: reduce) {
  * { transition-duration: 0ms !important; animation-duration: 0ms !important; }
}
```

## 5. Components (primitives)

### Button

Markup:

```html
<button class="btn btn--primary">Guardar escenario</button>
<button class="btn btn--secondary">Cancelar</button>
<button class="btn btn--ghost">Editar</button>
<button class="btn btn--danger">Borrar cuenta</button>
```

States: default / hover / focus-visible / active / disabled / loading.
Min target: 44×44 on `pointer: coarse`, 36×36 on fine.

### Form field

```html
<div class="field">
  <label for="ipo-price">Precio IPO (USD)</label>
  <input id="ipo-price" type="text" inputmode="decimal" />
  <p class="field__hint">Introduce el precio por acción en USD.</p>
  <p class="field__error" role="alert">El precio debe ser un número positivo.</p>
</div>
```

Error state links input to message via `aria-describedby`.

### Rule-set chip

A small pill showing the active rule set. Appears in the disclaimer footer and on each calculation result block.

```html
<span class="chip chip--ruleset" title="Computed under rule set es-2026.1.0, AEAT guidance 15 mar 2026">
  <span class="chip__dot" aria-hidden="true"></span>
  AEAT 15 mar 2026
  <span class="chip__version">es-2026.1.0</span>
</span>
```

Variants:
- `chip--ruleset` (default, neutral)
- `chip--ruleset chip--superseded` (warning color, for old exports)

### Range bar (the distinctive primitive)

CSS-only range bar with central dot. Uses custom properties for central/min/max as percentages.

```html
<div class="range" style="--range-min:0%; --range-max:100%; --range-central:55%;">
  <div class="range__rail" aria-hidden="true">
    <div class="range__band"></div>
    <div class="range__dot" title="Central estimate"></div>
  </div>
  <div class="range__labels">
    <span class="range__bound range__bound--low">€37.280</span>
    <span class="range__bound range__bound--high">€47.450</span>
  </div>
  <div class="range__driver">± 10 % precio IPO</div>
</div>
```

For SR: each `.range` carries `role="img"` and an `aria-label` describing low, central, high, and driver.

### Range headline (pattern C)

Used once per result page. The headline *is* the range.

```html
<div class="headline">
  <div class="headline__label">Neto en tu cuenta española</div>
  <div class="headline__range">
    <span class="headline__low">€98.400</span>
    <span class="headline__sep">—</span>
    <span class="headline__high">€112.600</span>
  </div>
  <div class="headline__central">Estimación central €105.500</div>
  <div class="range__rail" aria-hidden="true">...</div>
  <div class="headline__drivers">Precio ±10 % · FX spread 0–3 % · Impuesto estimado</div>
</div>
```

### Disclaimer footer

Persistent, one per content area. Not viewport-fixed (don't steal mobile real estate).

```html
<footer class="disclaimer">
  <span class="disclaimer__text">
    Esto no es asesoramiento fiscal ni financiero — Orbit calcula, no aconseja.
  </span>
  <span class="disclaimer__meta">
    <span class="chip chip--ruleset">AEAT 15 mar 2026 · es-2026.1.0</span>
    <button class="btn btn--ghost btn--sm">Ver trazabilidad</button>
  </span>
</footer>
```

### Formula trace panel

Expand/collapse under any clickable number.

```html
<button class="formula__toggle" aria-expanded="false" aria-controls="f-1">
  Ver fórmula
</button>
<div class="formula" id="f-1" hidden>
  <code class="formula__expr">(sell_price − cost_basis) × shares</code>
  <code class="formula__subst">($48,00 − $31,20) × 3.000 = $50.400 USD</code>
  <div class="formula__fx">→ €46.562 EUR (ECB 2026-04-18, 1 EUR = 1,0823 USD; spread 1,5 %)</div>
  <div class="formula__ruleset">Rule set es-2026.1.0 · AEAT 15 mar 2026</div>
</div>
```

### Alert block (Beckham / Foral / Modelo 720)

Non-dismissable informational block. Three variants:

```html
<aside class="alert alert--info">
  <strong>Régimen de impatriados (Beckham)</strong>
  <p>v1 no calcula resultados bajo este régimen. Consulta con tu asesor fiscal.</p>
</aside>

<aside class="alert alert--warning">
  <strong>Modelo 720 — umbral cruzado</strong>
  <p>Tu valor en activos extranjeros supera los €50.000 en la categoría de valores. Puede existir obligación de declarar. Orbit no presenta el Modelo 720.</p>
  <a href="#" class="alert__cta">Generar hoja de trabajo (PDF)</a>
</aside>
```

### Status badge

Small inline indicator paired with money or timestamps.

```html
<span class="badge badge--fresh">Actualizado 10:27 CET</span>
<span class="badge badge--stale">Cotización antigua · 17 min</span>
<span class="badge badge--override">Precio modificado manualmente</span>
```

### Table (money)

Semantic `<table>`. Right-align all money columns via CSS, not class.

```html
<table class="tbl tbl--money">
  <thead>
    <tr>
      <th scope="col">Grant</th>
      <th scope="col">Instrumento</th>
      <th scope="col" class="num">Acciones</th>
      <th scope="col" class="num">Vested</th>
      <th scope="col" class="num">Valor papel (EUR)</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <th scope="row">Refresh 2024-09</th>
      <td>RSU (double-trigger)</td>
      <td class="num">30.000</td>
      <td class="num">12.500</td>
      <td class="num">€387.500,00</td>
    </tr>
  </tbody>
</table>
```

## 6. Layout

- Sidebar: 240px fixed at ≥1024px; collapses to hamburger below.
- Content max-width: 1360px, centered.
- Grid: 12-col at desktop, 4-col at tablet, 1-col at mobile. Gutter 24px desktop, 16px tablet.

## 7. Conventions for numbers

See `orbit-v1-ui-proposal.md` §5.7. Summary:

- Money: `€42.318,50` (ES) / `€42,318.50` (EN). Currency suffix shown when mixing.
- Ranges: `€37.280 — €47.450` (em-dash).
- Negatives: `−€1.200` in `--color-negative`. Never parens.
- Percentages: `15 %` (RAE-style space).
- Zero-pad cents always.
- Tabular numerals always on money.

## 8. Accessibility rules baked into primitives

- Every `.btn` has visible `:focus-visible` ring using `--color-accent`, 2px width, 2px offset.
- Every `<input>` has an associated `<label>` (never a placeholder-as-label).
- Every disclosure (`.formula__toggle`, range expand) has `aria-expanded` + `aria-controls`.
- Status badges pair color with a leading icon or text prefix.
- `.disclaimer` is rendered as a `<footer>` and is in the landmark tree.
- Tables use `<th scope>`; money columns are `text-align: right` via CSS.
