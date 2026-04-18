# ADR-009: Frontend architecture

- **Status:** Proposed
- **Date:** 2026-04-18
- **Deciders:** Ivan (owner)
- **Traces to:** ADR-001 (SPA + Vite locked), ADR-008 (disclaimer-before-hydration constraint), UX proposal §1–§11, `style-guide.md`, spec §7.2 / §7.4 / §7.10, SEC-180 (strict CSP, no `'unsafe-inline'`), SEC-188 (CSRF double-submit), SEC-052 (analytics allowlist), Slice 1 ACs G-11..G-25 (i18n + a11y).

## Context

ADR-001 locks the macro choice: **React 18 SPA built with Vite, no SSR**. That ADR deferred the concrete frontend makeup ("likely TanStack Query + minimal Zustand") to the implementation pass. Three downstream documents have since tightened the constraints and we can now close them:

1. **UX proposal + style-guide** demand a Bloomberg-terminal-density aesthetic with tabular numerals, right-aligned money columns, a CSS-only range-bar primitive, dark-mode tokens, WCAG 2.2 AA, `prefers-reduced-motion` and `prefers-color-scheme` support, **ES-first bilingual** with locale-aware number + date formatting, and a deliberate absence of consumer-app flourishes (no Intercom widget, no confetti).
2. **Security requirements** impose a **strict CSP** with `script-src 'self'` and no `'unsafe-inline'` (SEC-180), CSRF double-submit headers (SEC-188), and a compile-time logging allowlist (SEC-050) that extends to client-side telemetry via SEC-052. These rule out several popular client-side libraries that inject runtime styles/scripts.
3. **Slice 1 ACs** require the sign-up wizard, vesting preview rendering live as the user types, ES/EN locale toggle, audit-log writes on residency and grant edits, and a CI lint that blocks PRs introducing a string without both locale catalogs.

This ADR closes the remaining open frontend choices (framework sub-choices, state, routing, forms, i18n, styling delivery, asset packaging) at the depth Slice 1 requires. Decisions are **reversible at Slice 2–3 boundaries** except the CSP and packaging posture — those are expensive to undo and are treated accordingly.

## Decision

### Stack

| Concern | Choice | Version floor |
|---|---|---|
| UI framework | React 18 (locked by ADR-001) | 18.3.x |
| Build tool | Vite (locked by ADR-001) | 5.x |
| Language | TypeScript in `strict` mode | 5.5.x |
| Router | **React Router v6** (data-router API: `createBrowserRouter` + loaders/actions) | 6.26.x |
| Server-state cache | **TanStack Query v5** (fka React Query) | 5.x |
| Client state (UI-only) | **Zustand** for the tiny amount of cross-component UI state (sidebar collapse, locale, disclaimer-modal-shown) | 4.5.x |
| Forms + validation | **React Hook Form** + **Zod** (resolver) + a thin shared `apiError → field` adapter | RHF 7.53.x, Zod 3.23.x |
| i18n | **LinguiJS v4** (`@lingui/core` + `@lingui/react`) with PO catalogs; locale-aware number/date via `Intl.NumberFormat` / `Intl.DateTimeFormat` directly (no moment/date-fns-tz in v1 — `Temporal` polyfill deferred) | 4.x |
| Styling | **Plain CSS + CSS custom properties**, authored as `shared.css` in the design package. No CSS-in-JS. No Tailwind. | — |
| Icons | **Lucide** imported as individual tree-shaken React components (`lucide-react`) | 0.445.x |
| Charting (vesting timeline, sensitivity bars) | **`<canvas>` + hand-rolled render** for the vesting cumulative curve + the Pattern-C range rail; **no chart library** in Slice 1. Revisit for Slice 4 sensitivity table if a library is justified. | — |
| Date/time | Native `Intl` APIs; storage is ISO 8601 string, parsed to `Date` only at display | — |
| Testing | **Vitest** (unit) + **Playwright** (E2E + `axe-core` accessibility smoke) | Vitest 2.x, Playwright 1.47.x |

### Rationale (per choice, one line unless load-bearing)

- **React Router v6 data-router** is the minimum abstraction that gives us per-route loaders, which map cleanly onto the slice-1 flows (signup wizard steps, grant-detail load, dashboard tiles). It composes with TanStack Query via loader-wrapping.
- **TanStack Query** is chosen over SWR on dev-tool maturity and mutation ergonomics. It owns the server-state cache, retry policy, and optimistic updates — the app needs all three (live vesting preview, grant edit, audit-log-visible changes).
- **Zustand** is load-bearing only for UI-state that doesn't belong in the URL and doesn't belong in the server cache. Intentionally tiny; a Redux-class store would be overkill for Slice 1 and a trap for Slice 4+ (every state blob in a global store is a PII leak surface — SEC-050).
- **React Hook Form + Zod.** RHF over Formik on re-render cost and uncontrolled-input story. Zod over Yup/Joi on TypeScript-first inference and on a shared schema story with backend (see ADR-010 §Validation). The first-grant form has conditional validation (strike required for NSO/ISO; `liquidity_event_date` only shown for double-trigger RSU); RHF+Zod handles this cleanly.
- **LinguiJS over react-intl / i18next.** Lingui's `t` macro compiles to pre-extracted PO catalogs at build time, which means: (a) no runtime locale-catalog parsing (bundle stays small), (b) a catalog-completeness lint is trivial to add (Slice 1 AC G-11), (c) plural/ICU support is built in without a runtime cost for simple strings. PO files are reviewable in diff; YAML-ish alternatives are not. Ivan being fluent in ES and EN removes the usual "translator hand-off" argument for tool-independence.
- **Plain CSS + custom properties.** The UX proposal explicitly wants a design that doesn't look like Tailwind or a consumer React app; `shared.css` already exists and uses tokens. CSS-in-JS (emotion, styled-components) would either require `'unsafe-inline'` in CSP (Emotion's default injects `<style>` tags at runtime) or a build-time extract step that adds a dependency for near-zero benefit. Tailwind is possible under strict CSP, but its atomic-class output conflicts with the right-aligned money column and `font-variant-numeric: tabular-nums` ergonomics the style guide demands at the semantic-class level (`.tbl--money`, `.num`). **Plain CSS is the cheapest thing that both satisfies CSP and preserves the style-guide markup as shipped.**
- **No chart library in Slice 1.** The vesting curve is a single-line cumulative plot with a dashed-fill variant for "time-vested awaiting liquidity". A hand-rolled `<canvas>` renderer (~150 LOC) avoids a 50–100 KB dependency, avoids the CSP issues some chart libs have (Plotly ships inline styles), and gives us exact control over the double-trigger visual (AC-4.3.4, AC-6.1.4). Recharts / Nivo become plausible at Slice 4 when sensitivity-table styling gets richer. Decision reversible.
- **Vitest + Playwright** are the Vite-idiomatic choices and both integrate `axe-core` for the Slice-1 a11y smoke test (G-21).

### Strict CSP compatibility (SEC-180)

The stack above boots under the exact CSP in SEC-180: `script-src 'self'`, `style-src 'self'`, no inline scripts, no inline styles. Specifically:

- Vite's production build emits linked `<script src>` and `<link rel="stylesheet">`; no inline tags.
- React does not require inline scripts.
- Plain CSS ships as a static file.
- No third-party runtime script tags. Stripe.js, when it arrives in Slice 3, is scoped to the `/billing/*` routes via a **route-specific CSP** (`connect-src https://api.stripe.com; script-src 'self' https://js.stripe.com;`) served by axum's per-route header middleware. The rest of the app keeps the tight default.
- Lingui catalogs are imported as JSON/JS modules at build time; no runtime `eval`, no dynamic `new Function`.
- `Intl.NumberFormat` is browser-native, no CSP impact.

A **nonce-based fallback** is prepared but **off by default**. The axum middleware that emits `Content-Security-Policy` can mint a per-request nonce if a future slice needs an inline script (e.g., a polyfill shim); no Slice-1 code uses it.

### Packaging of the design system

The UX proposal and `style-guide.md` currently live under `docs/design/`. For Slice 1 we **do not build a separate npm package**: those files are copied into the frontend source tree as `frontend/src/styles/tokens.css` and `frontend/src/styles/primitives.css`, imported from `frontend/src/main.tsx`. The HTML reference screens in `docs/design/screens/` remain the canonical design source; divergence is caught by a weekly visual-diff review rather than by a versioning dependency.

Rationale: a single-developer, single-frontend product does not need a package boundary. Reintroduce a package if a second frontend (marketing static site, admin console) ever needs the same tokens.

### Routing map (Slice-1 concrete)

```
/                            -> redirects based on auth + onboarding state
/signup                      -> signup wizard (step state in URL: /signup/password, /signup/verify-email, /signup/disclaimer, /signup/residency, /signup/first-grant)
/signin
/signin/forgot
/signin/reset/:token
/app                         -> authenticated shell (sidebar + footer)
/app/dashboard               -> Slice-1 single-/multi-grant tiles
/app/grants                  -> grants list
/app/grants/:grantId         -> grant detail + vesting timeline
/app/grants/new              -> add-grant form
/app/account/profile         -> residency edit
/app/account/privacy         -> DSR placeholders (Slice 7 fills these)
/app/*                       -> catch-all 404
```

Signup wizard state is URL-first (shareable, back-button correct, reload-safe); **no wizard state in a global store**. TanStack Query owns any server-state read during the wizard (e.g., the email-verified flag).

### State-management taxonomy

One decision-line per kind of state:

| State kind | Example | Owner |
|---|---|---|
| Server state (persisted) | grants, user, residency_periods | TanStack Query |
| URL state | wizard step, filter params, current grant id | React Router |
| UI-ephemeral (per component) | form inputs, expansion toggles | `useState` / RHF |
| UI-ephemeral (cross-component) | locale, sidebar collapse, disclaimer-acknowledged flag | Zustand (one store, ≤10 fields) |
| Derived | vested-to-date, vesting-events list | `useMemo` from grant data |
| Cached computation | (none in Slice 1) | — |

**No Redux, no RTK, no MobX, no Jotai, no Recoil.** Any need for those is evidence the state taxonomy was wrong.

### Analytics posture

Slice 0 ships with **analytics disabled by default** (G-27). When analytics are introduced in a later slice (not Slice 1), the client uses a **typed event builder** whose `emit(event)` rejects any field type that has not been schema-allowlisted (SEC-052). Self-hosted Plausible is the candidate (EU-hosted, no PII); decision deferred to the slice that needs it.

### Locale handling

- Locale switcher lives in the top bar and is also honored from `Accept-Language` on first load; user's choice persists in a `localStorage` key `orbit.locale` (not a cookie — avoids AEPD cookie-banner issue at Slice 0).
- Spanish **tax terms** remain in Spanish under every locale per G-12 (`IRPF`, `ahorro base`, `Modelo 720`, etc.). Lingui messages that embed these terms use a no-op wrapper marker (`<tax>IRPF</tax>`) so translators know the term is invariant; the `<tax>` component simply returns children.
- The non-advice **disclaimer** is rendered ES-first under every locale per the G-26-adjacent analyst constraint in C-2. EN users see the ES text followed by an EN gloss.
- Number formatting uses `Intl.NumberFormat('es-ES', ...)` / `('en-GB', ...)`. Currency is rendered with an explicit suffix (`$8.00 USD` in EN, `8,00 USD` in ES) per style-guide §7.

### Error handling + loading states

- TanStack Query + React Router error boundaries catch network / 5xx paths. Each route defines an `errorElement`.
- 401 returned by the API triggers a `/signin?returnTo=<path>` redirect (AC-7.2).
- 404 from a `Tx::for_user`-scoped fetch renders a "grant not found" state, **not a 403** — RLS is fail-closed, we do not leak existence (AC-7.3, SEC-023).
- Forms display inline field errors via `aria-describedby` and a centralized `<Field>` component (G-18).

### Testing matrix (Slice 1)

- Vitest unit tests on: vesting-derivation helpers (AC-4.3.1..4.3.5; also tested backend-side — see ADR-014), form schema validation (Zod), i18n catalog completeness.
- Playwright E2E: the 17-step demo-acceptance script from slice-1-acceptance-criteria.md §10, scripted.
- `axe-core` via `@axe-core/playwright` run on each of: signup wizard, residency step, grant form, dashboard, grant-detail (G-21).
- Visual regression: **not in Slice 1.** Revisit at Slice 4 when range-bar rendering becomes load-bearing.

## Alternatives considered

- **Next.js (App Router) with SSR.** Revisited despite ADR-001. Rejected a second time: SSR adds a Node runtime tier and a second deploy surface inside the Rust-single-binary topology (ADR-002) for zero Slice-1 benefit. The disclaimer-before-hydration concern that a less-mature ADR-001 cited is handled by the server-emitted `index.html` shell.
- **Remix.** Same SSR objection.
- **Solid / Svelte / Preact.** All cheaper per-render than React; none justifies the hiring-pool / documentation tax for a tool whose render bottleneck is not component overhead.
- **TanStack Router.** Plausible; tighter TS integration than React Router. Rejected on maturity relative to React Router v6 data APIs and on the larger community example corpus for the specific patterns used (wizard, protected routes). Revisit at v1.1.
- **Redux Toolkit.** Overkill. The server-state story is owned by TanStack Query; no Slice-1 UI state justifies a reducer tree.
- **Formik.** Acceptable but slower on large forms; RHF wins on the scenario modeler (Slice 4) that's coming.
- **react-intl / i18next.** Both bundle runtime catalog parsers; Lingui compiles. For a bilingual app with a CI lint on catalog completeness, compile-time wins.
- **Tailwind CSS.** Works under strict CSP; conflicts semantically with the style-guide's semantic classes and would duplicate the token layer already authored. Rejected.
- **Emotion / styled-components.** Require either `'unsafe-inline'` (CSP-hostile) or an extract-at-build pipeline we don't need. Rejected.
- **Recharts / Chart.js / Plotly / Nivo for Slice 1 vesting curve.** Each adds 40–120 KB for a single one-line chart. Rejected until Slice 4.
- **Storybook.** Useful but not Slice-1-blocking. Deferred.

## Consequences

**Positive:**
- Strict CSP boots on day one; no retrofit when Slice 3's billing screen adds Stripe.
- Bundle stays small (Vite tree-shaking + zero chart library + compiled Lingui catalogs) which helps the §7.8 ≤2 s P75 dashboard budget on EU broadband.
- The state taxonomy is explicit, which makes Slice 4 (scenario modeler) easier to design — no "which store owns scenario inputs" debate.
- i18n completeness is CI-enforced, matching AC G-11.
- The stack is hireable; every piece is well-documented.

**Negative / risks:**
- Hand-rolled vesting chart means Slice 1 engineers can't ship a chart by importing a library. Mitigation: the Slice-1 vesting curve is simple; reference implementation is ≤150 LOC (see ADR-014 §Vesting algorithm + render sketch).
- Plain CSS means no component-scoped styles; class-name collisions are a live risk. Mitigation: BEM-ish naming already used in `shared.css` (`.range__rail`, `.chip--ruleset`).
- Lingui's PO workflow requires a catalog-extract step on every PR; engineers new to it may stumble. Mitigation: pre-commit hook runs `lingui extract`.
- TanStack Query's cache persistence across tabs is off by default; the app will re-fetch on each tab open. Acceptable for Slice 1 traffic; revisit for Slice 4 scenario-modeler iteration.
- No Redux/RTK means the eventual scenario-modeler "duplicate scenario" flow (UX §4.3 step 5) must be modeled as two TanStack Query cache entries rather than a reducer tree. Intentional.

**Tension with prior ADRs:**
- None. ADR-001 left sub-choices open; this ADR fills them without contradiction.

**Follow-ups (not blocking Slice 1):**
- Implementation engineer: confirm Lingui `t` macro works with Vite's SWC plugin pipeline (it does at Lingui 4.x; worth a smoke test on day one).
- Implementation engineer: author `frontend/src/testing/a11y.ts` exporting a reusable axe-core harness for Playwright.
- Implementation engineer: decide between `lucide-react` direct imports and a sprite-compile step if the icon surface grows; Slice-1 icon count is small enough not to matter.
- Slice-4 follow-up: revisit chart library choice when the sensitivity table and the Pattern-C range rail on the scenario modeler arrive.
- Slice-3 follow-up: define the route-specific CSP rule for Stripe.js and document the delta.
