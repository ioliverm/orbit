# ADR-001: Tech stack baseline

- **Status:** Proposed
- **Date:** 2026-04-18
- **Deciders:** Ivan (owner)

## Context

Orbit v1 needs a baseline tech stack locked before downstream architectural ADRs can reference it without relitigation. The product is a low-traffic, calculation-heavy, regulated EU SaaS that must be cheap to run during validation (<€200/mo infra; see ADR-002), single-developer-friendly, and credible for handling Spanish-tax computations whose results carry real-money consequences.

Three constraints dominate the choice:

1. **Correctness over throughput.** Tax calculations must be deterministic, well-typed, easy to test exhaustively. Bugs cost users five-figure euros and cost Orbit reputation.
2. **Cost posture.** Single small VPS class. Rules out runtimes with heavy memory baselines or per-instance licensing.
3. **One developer.** No room for polyglot complexity. The same person writes API, workers, and migrations.

## Decision

- **Backend language:** **Rust** (stable channel). Web framework: `axum` (Tokio ecosystem, mature, idiomatic). Async runtime: Tokio.
- **Frontend:** **React 18 SPA** built with **Vite**, routed with **React Router**. No SSR. A tiny static marketing site lives alongside (plain HTML / minimal Astro or just hand-rolled — decided in implementation). The non-dismissable legal disclaimer is rendered in the **server-shipped HTML shell** so it lands before JS hydrates (ADR-008 covers traceability text; this ADR fixes the delivery mechanism).
- **Data store:** **PostgreSQL 16+** as the single system of record. Row-Level Security (RLS) for multi-tenancy (ADR-005). No other primary store in v1 — no Redis, no Elasticsearch, no separate analytics warehouse. Postgres `LISTEN/NOTIFY` or a `pg_cron`-driven job table covers the small amount of background work (ADR-002 elaborates on the worker process).
- **Deploy unit:** A **single Rust binary** with multiple entrypoints selected by CLI flag or env (`orbit api`, `orbit worker`, `orbit migrate`). Same image, different process role. This collapses build + deploy + observability surface area.

## Alternatives considered

- **Backend: Go / Elixir / TypeScript (Node).** Go is comparable on ops cost and faster to write, but Rust's type system and exhaustive matching on enums (instrument type, residency status, rule-set version) materially reduce the class of bugs that would cost users money. Elixir is excellent for concurrency Orbit doesn't need. Node was rejected on type-safety grounds for tax math.
- **Frontend: Next.js / Remix (SSR).** SSR adds an Node runtime tier — extra cost, extra deploy surface, extra failure mode — for no v1 benefit. Orbit is a logged-in calculation tool; SEO is irrelevant for the app, and the marketing site can be pure static. The disclaimer-before-hydration concern is handled by the server-rendered HTML shell (a pre-built `index.html` shipped by the API or CDN with the disclaimer baked in).
- **Data store: SQLite + Litestream.** Genuinely tempting at this scale and price point, but Postgres RLS for multi-tenant isolation (ADR-005) is a much stronger story than SQLite-level isolation, and ECB FX history + audit log + market-data cache + rule-set tables benefit from real concurrent writes from the worker.
- **Multi-binary services.** Rejected. One developer, one product, single-digit RPS — splitting into API service + worker service buys nothing and doubles ops cost.

## Consequences

**Positive:**
- Rust + Postgres + React is a boring, well-documented, hireable stack. No exotic runtime risk.
- Single binary keeps deploy, observability, and config trivially small. Same binary runs locally, in CI, and in prod.
- Strong types on the calculation core directly support the "every formula traceable" NFR (§7.4).
- React SPA + static marketing keeps frontend cost at CDN bandwidth only.

**Negative / risks:**
- Rust compile times are slow; CI iteration is the price. Mitigated by `cargo-chef` layer caching and small crate boundaries.
- React SPA means initial bundle size matters for the disclaimer-before-content NFR. Mitigated by server-shipping the disclaimer in the HTML shell, not waiting for JS.
- Rust hiring pool is smaller than Go/TS if Orbit ever scales the team. Acceptable risk at validation stage.
- Single-binary multi-role means a deploy of the API also redeploys the worker. Acceptable at this scale; revisit at v2.

**Follow-ups:**
- Pin Rust toolchain version in `rust-toolchain.toml` once implementation starts.
- Decide PDF rendering library in implementation pass (likely `typst` or headless Chromium — ADR-008 will note this).
- Confirm React state-management choice (likely TanStack Query + minimal Zustand) in implementation pass; not architecture-significant.
