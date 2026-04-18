# ADR-010: API contract shape

- **Status:** Proposed
- **Date:** 2026-04-18
- **Deciders:** Ivan (owner)
- **Traces to:** ADR-001 (axum backend, React SPA), ADR-005 (RLS model), ADR-009 (React Router + TanStack Query + Zod), spec §7.9, SEC-006 (cookie flags), SEC-020..SEC-026 (tenant isolation), SEC-051 (error envelope), SEC-160 (rate limits), SEC-187 (CORS), SEC-188 (CSRF double-submit), Slice 1 ACs 4.1 / 4.2 / 6 (residency, grant CRUD).

## Context

ADR-001 commits to axum as the backend and a React SPA as the only first-party client. No ADR yet commits to **REST vs GraphQL vs tRPC**, to an **auth transport** (cookie vs `Authorization` header), to an **error envelope**, to a **versioning strategy**, or to whether the contract is **OpenAPI-first or code-first**. The security requirements lean heavily toward cookies (SEC-006, SEC-188 assumes a CSRF double-submit pattern), but that needs to be confirmed, not assumed.

The product is small. Slice 1 needs at most ~15 endpoints. v1 end-state is perhaps 40–60. One developer authors both the backend and the frontend. There is no external API consumer in v1 and none scheduled for v1.1. The cost of a heavyweight contract framework is real; the cost of a badly-shaped contract is also real, particularly because every RLS-scoped path must be authenticated and cross-tenant-probe-tested (SEC-023).

## Decision

### 1. Style: JSON-over-HTTP REST

**REST with pragmatic verbs over a small set of resources.** No GraphQL, no tRPC in v1.

Reasoning:

- **GraphQL** is rejected on two counts. First, the app's data shape is tabular and already well-normalized; there is no Relay-style nested fetch the SPA needs that justifies GraphQL's complexity budget. Second, authz in a GraphQL schema with N resolvers becomes a per-field RLS concern — the `Tx::for_user` + RLS model (SEC-022, SEC-023) maps cleanly to per-endpoint axum handlers and awkwardly to per-field resolvers.
- **tRPC** is rejected on "the client is not Node and we don't want to couple Rust types to a TS-only contract generator." We could do the same thing via OpenAPI + code-gen; see §4.
- **REST** matches how the SPA already thinks about the data (`/grants/:id`, `/residency`), matches the axum idiom, and lets each handler own its own authz check explicitly.

### 2. Base URL and routing

- SPA served at `https://app.orbit.<tld>` (Bunny.net CDN fronting static assets from ADR-002).
- API served at `https://app.orbit.<tld>/api/v1/...` — **same origin as the SPA.** Not a separate `api.orbit.<tld>` subdomain.

Rationale for same-origin:

- Cookies work without any cross-site dance. `SameSite=Lax` is sufficient (SEC-006); no need for `SameSite=None`.
- CORS is unnecessary for the first-party client. The CORS policy specified in SEC-187 applies only to pre-flighted cross-origin dev tools and to any future non-app client; the default behaviour is "don't cross origins in production."
- CSRF double-submit (SEC-188) is simpler when we control both origins.

CDN behaviour: Bunny.net serves `/assets/*` from edge cache; `/api/*` passes through to Caddy (no caching). The `app.orbit.<tld>` apex serves the SPA shell `index.html` with CSP and other security headers from SEC-180..SEC-189.

### 3. Versioning

**Path-based major version: `/api/v1/...`.**

- Additive changes (new endpoints, new fields on responses) do not require a version bump.
- Breaking changes (renamed fields, removed endpoints, changed types) require `/api/v2/...`. v2 can live side-by-side with v1 during a migration window.
- **Header-based versioning (`Accept: application/vnd.orbit.v2+json`) is rejected** on ergonomics: it makes `curl`-debugging worse, makes CDN cache keys awkward, and the ritual cost of `v1 → v2` in a single-client app is negligible compared with the clarity win.

Slice-1 reality: the API stays at `/api/v1/...` through the full v1 product. The versioning is there to make **v1.1 and v2** additions cheap to document, not to enable breaking-change churn within v1.

### 4. OpenAPI-first (contract-first, but pragmatic)

**OpenAPI 3.1 YAML is the source of truth for the contract**, maintained at `backend/api/openapi.yaml`. The backend generates types via **`utoipa`** annotations on axum handlers (code-first annotations → emitted OpenAPI) **and** the emitted YAML is checked in.

Workflow:

1. Engineer adds or changes an axum handler. `utoipa` derives the OpenAPI fragment from the handler and its request/response DTOs.
2. CI runs `cargo xtask openapi-dump` → writes `backend/api/openapi.yaml`. The check-in is diff-reviewable.
3. CI runs `backend/api/openapi.yaml` through `openapi-typescript` → writes `frontend/src/api/generated.ts` (types only, not a client — see below). Committed.
4. The SPA imports types from `generated.ts` and calls endpoints via a hand-written **tiny** `apiFetch<T>(path, init)` helper (~60 LOC) that handles: base URL, CSRF header injection, error-envelope parsing, 401 redirect.

Rationale: "OpenAPI-first" in the pure sense (write YAML, generate server stubs) is heavyweight; "code-first with emitted OpenAPI that becomes source of truth" (Spring-ish / FastAPI-ish) is what actually works for a single developer. `utoipa` is a well-established crate and its output is deterministic enough to diff.

We do **not** generate a full client with `openapi-typescript-codegen` or `orval`. The generated client is usually larger than the hand-rolled helper and is harder to integrate cleanly with TanStack Query. Types are enough; the call-site stays readable.

### 5. Auth transport: cookies

**Session cookies.** Not `Authorization: Bearer`.

- Access token cookie: `orbit_sess` — `HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=1800` (30 min). Contains an opaque session id (not a JWT); resolved server-side against the `sessions` table (ADR-005).
- Refresh cookie: `orbit_refresh` — `HttpOnly; Secure; SameSite=Lax; Path=/api/v1/auth; Max-Age=604800` (7 days). Opaque refresh id; hash stored as `sessions.refresh_token_hash`. Rotates on every use (SEC-006); re-use of an old refresh triggers revocation of the full session family.
- CSRF cookie: `orbit_csrf` — **NOT** `HttpOnly` (the SPA reads it to mirror into the header). `Secure; SameSite=Lax; Path=/`. Random 32-byte token. Rotated on signin.
- SPA sends `X-CSRF-Token: <value of orbit_csrf cookie>` on every state-changing request. Backend asserts `header == cookie` on `POST|PUT|PATCH|DELETE`. (SEC-188, double-submit.)

**Why not JWT in `Authorization` header:**

- JWTs in `localStorage` are XSS-loot; in a cookie they lose their one real advantage (stateless revocation is already non-existent server-side once we need revocation — which we do per SEC-010).
- An opaque session id matched to a DB row gives us: instant revocation (session.revoked_at), device list (SEC-010), refresh rotation detection, IP-hash tracking — all SEC-010 + SEC-006 requirements.
- The only real argument for JWT is "stateless scaling across many API instances." We have **one** API instance (ADR-002). Revisit if that changes at v2.

### 6. Request / response conventions

- **Content type:** `application/json; charset=utf-8` on every request and response body. No `multipart/form-data` in Slice 1 (CSV upload lives in Slice 2; that endpoint will accept `text/csv` or `multipart`).
- **Naming:** `camelCase` in JSON bodies (matches TypeScript idiom on the SPA side). Rust serde uses `#[serde(rename_all = "camelCase")]` globally on request/response DTOs. DB column names stay `snake_case`; the DTO layer is where the conversion happens.
- **IDs:** always UUIDv4 strings. No numeric IDs in wire format.
- **Money:** always an object `{ "amount": "42318.50", "currency": "EUR" }` — **`amount` is a decimal string**, never a JSON number (floats would violate the tax-engine's decimal discipline, SEC-085). The SPA parses to `Decimal.js` for arithmetic and formats via `Intl.NumberFormat`.
- **Dates:** ISO 8601 strings. Date-only fields as `YYYY-MM-DD`; timestamps as `YYYY-MM-DDTHH:MM:SS.sssZ` (UTC).
- **Pagination:** cursor-based. Response shape:
  ```json
  { "items": [...], "nextCursor": "opaque-string-or-null" }
  ```
  Slice 1 never paginates (user has at most ~20 grants); the contract still includes `nextCursor: null` from day one so the SPA doesn't break when Slice 2 adds real pagination.
- **Rate-limit headers:** `X-RateLimit-Limit`, `X-RateLimit-Remaining`, `X-RateLimit-Reset` on every authenticated response (SEC-160).
- **Request correlation:** every response carries `X-Request-Id: <uuid>` echoed from the inbound header or generated server-side. Logged (SEC-055) and shown to the user on error pages.

### 7. Error envelope (SEC-051)

Uniform across every endpoint. **Never a stack trace, never an echo of user input, never a grant value.**

```json
{
  "error": {
    "code": "grant.invalid.cliff_exceeds_vesting",
    "message": "El cliff no puede superar el periodo total de vesting.",
    "messageEn": "The cliff cannot exceed the total vesting period.",
    "field": "cliffMonths",
    "requestId": "018f3b7e-7e3c-7a5e-a7f0-b1d0c0e1f2a3"
  }
}
```

- `code` is a **dot-delimited, stable, lowercase** identifier the SPA switches on. Never renamed; new variants added.
- `message` and `messageEn` are pre-translated by the server for consistency (the SPA doesn't need a second locale catalog for error messages). The server chooses based on the user's locale cookie or `Accept-Language` fallback.
- `field` is optional; present on 422 validation errors and references the `camelCase` field in the request body.
- Multi-field validation errors use `"errors": [ {code, message, messageEn, field}, ... ]` at the top level instead of a single `error`. The SPA's RHF adapter consumes this shape.
- `requestId` for support debugging. Identical to `X-Request-Id`.

HTTP status mapping:

| Status | Meaning | Typical codes |
|---|---|---|
| 200 / 201 / 204 | Success | — |
| 400 | Malformed JSON, missing required field at the envelope level | `request.malformed` |
| 401 | Unauthenticated — triggers SPA redirect to signin | `auth.unauthenticated` |
| 402 | Paid feature without entitlement (SEC-025) | `subscription.required` |
| 403 | Authenticated but forbidden (rare with RLS; used for admin surfaces) | `access.denied` |
| 404 | Resource not found **or** owned by another tenant (SEC-023, AC-7.3) | `resource.not_found` |
| 409 | Conflict (e.g., email already registered) | `user.email_exists` |
| 422 | Validation error on a well-formed request | `grant.invalid.*`, `residency.invalid.*`, etc. |
| 429 | Rate limit exceeded | `rate_limit.exceeded` |
| 500 | Unexpected server error | `server.internal` (no details leaked) |
| 503 | Compute budget exceeded, vendor unavailable | `compute.timeout`, `vendor.unavailable` |

### 8. Validation

- Server-side: **`validator`** crate + `serde` on DTOs. Every field has explicit bounds (SEC-163 length caps, ticker regex, numeric ranges). Invalid inputs return **422** with the error envelope above.
- Client-side: **Zod** schemas per form (ADR-009). The Zod schema and the Rust `validator` rules must stay in sync manually; a Slice-2 follow-up may explore sharing a JSON Schema (emitted by `schemars`) but Slice 1 has too few forms to justify that machinery.

### 9. Slice-1 endpoint surface

The minimal contract Slice 1 needs. Path-relative to `/api/v1`. `[A]` = authenticated; `[G]` = guest; `[V]` = CSRF-validated state change.

**Auth / session**

| Method | Path | Notes |
|---|---|---|
| `POST` | `/auth/signup` `[G]` `[V]` | Email + password. Returns 201 + `orbit_sess` cookie. Triggers verification email. Rate limit per SEC-160. |
| `POST` | `/auth/verify-email` `[G]` `[V]` | Body: `{ "token": "..." }`. Marks email verified. |
| `POST` | `/auth/signin` `[G]` `[V]` | Body: `{ email, password }`. 200 + session/refresh/csrf cookies. Generic error per SEC-004. |
| `POST` | `/auth/mfa/challenge` `[V]` | **Scaffolded in Slice 1, not reachable.** The contract is defined here so Slice 7's TOTP mandate is an additive flip. Body: `{ "code": "123456" }`. Returns 200 on success; 401 on failure. |
| `POST` | `/auth/refresh` `[V]` | Rotates refresh. No body. Cookies only. |
| `POST` | `/auth/signout` `[A]` `[V]` | Revokes the current session (audit-logged). |
| `POST` | `/auth/forgot` `[G]` `[V]` | Body: `{ email }`. Always returns 200 + generic message (SEC-004). |
| `POST` | `/auth/reset` `[G]` `[V]` | Body: `{ token, newPassword }`. 60-min token (SEC-005). |
| `GET`  | `/auth/me` `[A]` | Returns user + residency + entitlement summary. Called by the SPA on app boot. |

**Residency / profile**

| Method | Path | Notes |
|---|---|---|
| `GET`  | `/residency` `[A]` | Returns current residency period. |
| `POST` | `/residency` `[A]` `[V]` | Creates a new residency period; closes prior (AC-4.1.7). |

**Grants**

| Method | Path | Notes |
|---|---|---|
| `GET`  | `/grants` `[A]` | List grants for current user (RLS-scoped). |
| `POST` | `/grants` `[A]` `[V]` | Create grant. Returns 201 + grant + derived vesting events. |
| `GET`  | `/grants/:id` `[A]` | Fetch one. 404 if not owned (AC-7.3). |
| `PATCH` | `/grants/:id` `[A]` `[V]` | Update. Recomputes vesting events. |
| `DELETE` | `/grants/:id` `[A]` `[V]` | Hard delete (AC-6.2.4). |
| `GET`  | `/grants/:id/vesting` `[A]` | Returns list of derived vesting events for rendering the timeline. |

**Consent / disclaimer**

| Method | Path | Notes |
|---|---|---|
| `POST` | `/consent/disclaimer` `[A]` `[V]` | Records disclaimer acceptance in `audit_log` (G-9). Idempotent (second call is a no-op). |

**Ops (unauthenticated, health only)**

| Method | Path | Notes |
|---|---|---|
| `GET`  | `/healthz` `[G]` | 200 if the process is up. No DB check. |
| `GET`  | `/readyz` `[G]` | 200 if DB reachable + RLS policies present (runs the cheap `SELECT 1 FROM pg_policies WHERE ...` probe). |

**Not in Slice 1** (contracts designed later; listed so the shape is visible):

- Billing (`/billing/*`) — Slice 3.
- Scenarios (`/scenarios`, `/scenarios/:id/run`) — Slice 4.
- Sell-now compute (`/sell-now/compute`) — Slice 5.
- Market quote (`/market/quote/:ticker`) — Slice 5.
- Exports (`/exports`, `/verify/:traceabilityId`) — Slice 6.
- DSR (`/dsr/export`, `/dsr/delete`, `/dsr/restrict`, `/dsr/rectify`) — Slice 7.
- CSV import (`/grants:import`) — Slice 2.

### 10. Extensibility shape

Three patterns that keep the v1 surface small but extensible:

1. **Resource-then-action** URLs for non-CRUD ops: `/scenarios/:id/run`, `/grants:import` (colon separator is a stable convention; we pick colon over slash here because `run` is an action on a specific scenario, not a sub-resource collection). Slice 1 only uses CRUD; the convention is locked so Slice 4/5 doesn't invent something new.
2. **Additive-only field evolution.** Responses never remove fields within v1. Optional fields default to `null` in generated types and the SPA treats unknown new fields as non-breaking.
3. **Feature-flagged endpoints.** When a Slice ships an endpoint still behind a paid-tier gate, the endpoint returns 402 with `subscription.required` for non-entitled users rather than 404. This keeps the surface testable from day one.

### 11. OpenAPI file location and lifecycle

- `backend/api/openapi.yaml` — generated by `utoipa`, committed. Reviewable in PR diffs.
- `frontend/src/api/generated.ts` — emitted by `openapi-typescript`, committed.
- Both files are CI-regenerated on every PR; a drift between what the handlers declare and what's committed fails the build.

## Alternatives considered

- **GraphQL (async-graphql or juniper).** Rejected on authz ergonomics (per-field RLS would force either a schema full of `@auth` directives or a data-loader pattern that breaks the `Tx::for_user` abstraction). The client-side flexibility win is irrelevant when there is one client.
- **tRPC.** Rejected — Rust backend makes this a non-starter without an intermediary Node gateway, which defeats the simplicity argument.
- **Cap'n Proto / gRPC-Web.** Ruled out on binary-debuggability and on tooling cost.
- **JWT in `Authorization` header (stateless).** Rejected on revocation story (SEC-010 needs session-list + revoke, which pushes state to the server anyway).
- **Header-based API versioning.** Rejected on ergonomics.
- **Full code-gen client (openapi-typescript-codegen).** Larger and less TanStack-Query-friendly than hand-rolled. Rejected.
- **Keep the OpenAPI file free-form / don't generate it.** Rejected — security review (SEC-023) and engineer onboarding both benefit from a single list of endpoints with auth and RLS annotations.
- **Separate subdomain `api.orbit.<tld>`.** Rejected on cookie and CORS complexity gain vs zero ops benefit at this scale.

## Consequences

**Positive:**
- The SPA–API coupling is clean: same origin, cookie auth, double-submit CSRF, typed envelopes.
- One API version lives for the entire v1 lifetime; `/api/v2/` is a deliberate option, not a routine occurrence.
- Error shape is uniform; RHF adapter consumes it mechanically.
- The OpenAPI file is a code-review artifact, which helps SEC-023 cross-tenant probe writing and SEC-200 security-review-gate PRs.
- Money-as-decimal-string keeps the frontend from ever `parseFloat()`-ing a tax-relevant number.

**Negative / risks:**
- Cookie auth means future non-browser clients (mobile app, CLI) need an `Authorization` path added — an acceptable v2 cost; not needed in v1.
- The `utoipa` workflow has a small learning cost; the committed-YAML + CI-drift check is what keeps it honest.
- Zod ↔ `validator` duplication is real. Mitigation: keep both schemas in the same PR and add a small test that exercises the "valid in Zod but invalid on server" edge in Slice 4.
- Rate-limit store is Postgres-backed (SEC-160) rather than Redis; at Slice 5 burst rates this is acceptable but not optimal. Revisit if Postgres contention is measurable.

**Tension with prior ADRs:**
- None.

**Follow-ups:**
- Implementation engineer: author `backend/api/openapi.yaml` skeleton and wire the `utoipa` derive macros on the Slice-1 handlers.
- Implementation engineer: write `frontend/src/api/fetch.ts` (~60 LOC) as the single outbound call site; integration-test that 401 triggers redirect and 422 surfaces to RHF.
- Implementation engineer: write the cross-tenant probe suite (SEC-023) against every `[A]` endpoint listed above before Slice-1 sign-off.
- Slice-3 follow-up: Stripe webhook handler lives at `/api/v1/billing/webhooks/stripe`; signature verification per SEC-026.
- Slice-4 follow-up: `POST /api/v1/scenarios/:id/run` — response shape mirrors `TaxResult` (ADR-003) and includes the `inputs_hash` / `result_hash` fields so the SPA can surface them (ADR-008 + SEC-086).
