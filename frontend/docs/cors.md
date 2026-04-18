# CORS requirements for `orbit-api` (Slice 0a handoff)

**Status:** Advisory (frontend → backend). Do NOT implement client-side.

**Traces to:** `docs/security/security-checklist-slice-0.md` S0-14 · SEC-187 · ADR-015 §S0-14 (0a uses `http://localhost:5173`).

## Contract

When `orbit-api` boots, the backend must attach an [axum `CorsLayer`](https://docs.rs/tower-http/latest/tower_http/cors/index.html) with the following configuration:

| Header | Value | Notes |
|---|---|---|
| `Access-Control-Allow-Origin` | `http://localhost:5173` | Exact origin — the Vite dev server (ADR-015). **Do not** use `*` (incompatible with credentials). |
| `Access-Control-Allow-Credentials` | `true` | Session cookie (HttpOnly, SameSite=Lax) is in-scope per SEC-006 / SEC-188. |
| `Access-Control-Allow-Methods` | `GET, POST, PATCH, DELETE` | Matches the ADR-010 verb set. `PUT` and `OPTIONS` are not used. |
| `Access-Control-Allow-Headers` | `Content-Type, X-CSRF-Token` | Double-submit CSRF token per SEC-188. |
| `Access-Control-Max-Age` | `600` | Reasonable preflight cache window; tune with metrics. |

## Wire-up location

Land this at the **first API endpoint task** (earliest backend slice that exposes an HTTP route), not sooner. Until then, the Vite dev server's `/api` proxy keeps the frontend on a same-origin request path, so CORS is not exercised in Slice 0a verification.

## Origins for 0b and beyond

Slice 0b (`ADR-015` deploy-green) replaces the local origin with the production origin:

- `https://app.orbit.<tld>` — single entry; no localhost; no wildcard subdomains.

When the production origin is added, keep `http://localhost:5173` in the allowlist only in the **local-dev** configuration profile; it must not ship to production per SEC-187.

## Sketch

```rust
// orbit-api: attach once, wrap the router before serving.
use axum::http::{header, HeaderValue, Method};
use tower_http::cors::CorsLayer;

let cors = CorsLayer::new()
    .allow_origin("http://localhost:5173".parse::<HeaderValue>()?)
    .allow_credentials(true)
    .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
    .allow_headers([header::CONTENT_TYPE, HeaderName::from_static("x-csrf-token")])
    .max_age(std::time::Duration::from_secs(600));

let app = Router::new()
    // ... routes ...
    .layer(cors);
```

## Verification

`curl -I -H 'Origin: http://localhost:5173' http://127.0.0.1:3000/api/v1/healthz` must echo `Access-Control-Allow-Origin: http://localhost:5173` and `Access-Control-Allow-Credentials: true`.
