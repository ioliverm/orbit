# Orbit v1 — Security requirements

| Field       | Value                                                      |
|-------------|------------------------------------------------------------|
| Version     | 0.1-draft                                                  |
| Date        | 2026-04-18                                                 |
| Status      | Draft — implementation-ready                                |
| Owner       | security-engineer                                           |
| Companion   | `threat-model.md`, `security-checklist-slice-0.md`          |

Every requirement is **testable** and **numbered**. Requirements marked `(v1-blocker)` must be in place before any user signs up. `(v1-paid)` must be in place before any paying user. `(v1.1)` are deferred. Each requirement cites the threats (`Sxx`) and ADRs / spec sections it traces to.

---

## 1. Authentication & session

**Stack pick (opinionated):** Email + password (argon2id) with **TOTP 2FA optional for free, mandatory for paid in v1.1**, nudged aggressively at upgrade. WebAuthn/passwordless deferred to v2. Rationale: persona is technical and will use a password manager; TOTP is ubiquitous and does not introduce a device-attestation dependency; WebAuthn introduces recovery complexity that exceeds v1 budget. Paid-mandatory MFA is deferred 6 months per OQ-01 to avoid friction during validation, but the architecture supports toggling the mandate centrally.

- **SEC-001** `(v1-blocker)` Passwords hashed with **argon2id**, m=19456 KiB, t=2, p=1 (OWASP 2024 defaults) or stronger. Password storage never uses bcrypt/scrypt/SHA. *Test: parameters verifiable from a stored hash string; unit test pinned.* (S1, §7.2)
- **SEC-002** `(v1-blocker)` Password policy per NIST SP 800-63B: 12+ chars, no composition rules, check against a **breached-password list** (HIBP k-anonymity API) on signup and on password change. *Test: signup with `Password1!` from a dump rejected; signup with a randomly-generated 12-char passphrase accepted.* (S1)
- **SEC-003** `(v1-blocker)` **Rate limiting**: per-IP 10 signin attempts / 10 min; per-account 5 / 10 min then exponential backoff. Reset-password: 3 / email / hour. Signup: 5 / IP / hour. CAPTCHA challenge after 3 consecutive failures. *Test: scripted 11th signin attempt from the same IP returns 429.* (S1, §7.9)
- **SEC-004** `(v1-blocker)` Generic signin error message — "Credenciales inválidas" — identical for unknown-user, wrong-password, and locked-account. Password-reset confirmation identical regardless of whether the email is known. *Test: response body + timing within 50ms equivalent.* (S1, S2)
- **SEC-005** `(v1-blocker)` Password-reset token: **32-byte CSPRNG**, single-use, **60-minute expiry**, invalidated on password change and on MFA-setting change. Reset link transmitted only via email; never logged. *Test: reused token returns 400; expired token returns 400.* (S2)
- **SEC-006** `(v1-blocker)` Session cookies: `HttpOnly`, `Secure`, `SameSite=Lax` (or `Strict` if UX allows). Access token lifetime ≤30 min; refresh token rotates on each use; refresh rotation detection (re-use of an old refresh ⇒ revoke entire session family). Refresh stored as `refresh_token_hash` in `sessions` (ADR-005); revocable server-side. *Test: set all three flags; replayed refresh revokes session.* (S3)
- **SEC-007** `(v1-blocker)` Session ID rotation on signin, MFA step-up, password change, MFA change. *Test: pre-signin session cookie does not authenticate post-signin.* (S4)
- **SEC-008** `(v1-paid)` **TOTP 2FA** offered to all users, nudged at paid upgrade and at first export. TOTP seed generated server-side, 20-byte CSPRNG, shown once via QR and text; **stored column-encrypted** (libsodium `secretbox` or Postgres `pgcrypto`; key in secrets store, not in DB). Backup codes: 10 single-use codes, hashed with argon2id. *Test: TOTP seed column never appears plaintext in `pg_dump`.* (S6)
- **SEC-009** `(v1-blocker)` Password reset does NOT bypass MFA. MFA removal requires current-session re-auth plus a **24-hour cool-down**, with an email notice to all known addresses; cool-down abortable from any authenticated session. *Test: attempt to disable MFA returns a 24h-scheduled action + email.* (S5)
- **SEC-010** `(v1-blocker)` Users see an "Active sessions" panel listing `sessions` rows (device/IP-hash fingerprint) and can revoke any session. New-device-signin notification emailed to user's address. *Test: revoked session refresh fails; new-device email contains no PII beyond user-agent summary.* (S3, S7)
- **SEC-011** `(v1-paid)` Mandatory-MFA toggle for paid-tier users — **v1.1**, architected in v1 so the switch is a single config flag. (S8, OQ-01)

---

## 2. Authorization & tenant isolation

**Pattern:** Postgres Row-Level Security (ADR-005) with `USING + WITH CHECK` on `user_id = current_setting('app.user_id')::uuid` for every `[RLS]` table, enforced via the application's `orbit_app` non-superuser role.

- **SEC-020** `(v1-blocker)` Every table containing user-scoped data (`[RLS]` per ADR-005) MUST have RLS enabled and a policy covering both `USING` and `WITH CHECK`. Missing policy ⇒ CI fails via a DB-introspection test. *Test: pg_catalog query lists every user-scoped table with a non-null `rowsecurity=true` and a matching policy.* (S9)
- **SEC-021** `(v1-blocker)` The application connects exclusively as `orbit_app`, which is **not a superuser and does not hold `BYPASSRLS`**. Migrations run as a separate role. *Test: `SELECT rolbypassrls FROM pg_roles WHERE rolname='orbit_app'` returns `false`.* (S9)
- **SEC-022** `(v1-blocker)` The only sanctioned query-handle acquisition path is `Tx::for_user(user_id)`, which issues `SET LOCAL app.user_id = $1` inside the transaction. Direct `pool.acquire()` or `pool.begin()` outside this helper is **forbidden by a CI lint** (custom Clippy pass or `cargo-deny`-equivalent check). *Test: PR adding a direct `pool.acquire()` in a handler fails CI.* (S10)
- **SEC-023** `(v1-blocker)` Integration test suite includes **cross-tenant probes**: user A attempts to read/update/delete each `[RLS]` resource of user B via each exposed API endpoint; every probe MUST return 404 (not 403 — no existence disclosure) or empty collection. *Test: probe matrix runs in CI on every PR.* (S9)
- **SEC-024** `(v1-blocker)` Worker tasks operating on user-scoped data set `app.user_id` per work item. Cross-user system jobs (rule-set ingest, ECB fetch, retention sweep) operate only on non-RLS reference tables; any exception is code-reviewed and documented with a comment block. *Test: task-runner initialization asserts `app.user_id` is set before handing control to a user-scoped task.* (S11)
- **SEC-025** `(v1-blocker)` Paid-feature entitlement is derived server-side from `subscriptions.status` + `subscriptions.current_period_end`, not from client-supplied claims. Every paid endpoint starts with an entitlement check helper. *Test: forged JWT/cookie claiming paid returns 402 on paid endpoints.* (S12)
- **SEC-026** `(v1-blocker)` Billing webhook signatures are verified (Stripe `Stripe-Signature`, Paddle equivalent) before any state change; unsigned or invalid-signature webhooks 400 and are not retried from our side. *Test: unsigned webhook returns 400; state unchanged.* (S12)

---

## 3. Secrets management

**Provider:** In v1, OS-level secret file (0600, `orbit` user only) loaded at process start via a typed config struct. Deploy secrets live in **GitHub Environments** with required reviewers on `production`. No cloud KMS in v1 (cost; ADR-002). A managed secrets service (AWS Secrets Manager EU, Bitwarden Secrets Manager, Doppler) is a v1.1 revisit once operator count > 1.

- **SEC-030** `(v1-blocker)` **Nothing sensitive in source control**: passwords, API keys, JWT signing keys, DB credentials, TLS private keys, TOTP encryption key, backup encryption key. Enforced by `gitleaks` as a pre-commit hook and in CI on every push; history scanned at deploy. *Test: PR adding `DATABASE_URL=postgres://...` fails CI.* (S55)
- **SEC-031** `(v1-blocker)` Secret file world-unreadable; owned by the `orbit` user; loaded via `systemd` `LoadCredential=` directive on VM-1. *Test: `stat` on the secret file shows `0600 orbit orbit`.* (S55)
- **SEC-032** `(v1-paid)` **Rotation cadence**: database passwords and JWT signing keys annually; market-data vendor API key annually; email-provider API token annually; on suspected compromise, immediately and incident-logged. Rotation procedure documented in a runbook. *Test: runbook reviewed and dated within last 12 months.* (S29, S55)
- **SEC-033** `(v1-blocker)` JWT signing keys generated once per environment (separate keys for dev / staging / prod); key-ID in token header supports graceful rotation (accept old + new for overlap window). *Test: two-key-set rollover exercise passes.* (S54)
- **SEC-034** `(v1-blocker)` Outbound vendor API keys stored with a scoped principle — one key per external service, named with a human-readable owner comment in the secret file; rotated when the responsible person changes. (S29)
- **SEC-035** `(v1-blocker)` **TOTP encryption key** and **backup-bundle encryption key** are distinct from DB credentials and from each other; stored in the secret file; never logged; never included in backups (the backup key encrypts the backup — chicken-and-egg mitigated by storing the backup key in the secrets manager, not on the backup volume). *Test: decrypt/encrypt round-trip in a unit test with a fixture key.* (S6, S17)

---

## 4. Data classification & handling

Five classes. Every column and every log field belongs to exactly one.

| Class | Contents | Storage rule | Logging rule |
|---|---|---|---|
| **Public** | Marketing content, `/changelog/tax-rules`, rule-set data | Anywhere | Freely loggable |
| **Internal** | Application metrics, rate-limit counters, feature-flag state | In the primary DB; EU-hosted services OK | Loggable; no user linkage |
| **Personal (P)** | `users.email`, `users.locale`, IP address (hashed), user-agent | `[RLS]`-scoped; encrypted in transit; retained per §7.2 | **Never logged raw.** IP only as `ip_hash` with a long-lived salt in secrets. |
| **Financial-Personal (FP)** | Grants (share count, strike, ticker, employer, notes), scenarios, calculations, sell-now inputs, tax identifiers (NIF/NIE), Art. 7.p trip details, FX overrides, computed tax outputs | `[RLS]`-scoped; NIF/NIE column-encrypted; backups separately encrypted; exports 7-year retained (ADR-008) | **Forbidden in any log, event payload, crash report, or error response body.** See SEC-050. |
| **Regulatory-Critical (RC)** | Password hashes, MFA seeds, session tokens, rule-set content + hashes, audit log, operator credentials | Column-encrypted where feasible; write-once for rule-sets + audit log; 6-year retention for audit log | Never logged; access-restricted; tamper-evident (trigger on rule-sets, append-only on audit log) |

- **SEC-040** `(v1-blocker)` A data-classification comment block precedes every `CREATE TABLE` migration identifying the class of each column. *Test: every new migration PR reviewed against this convention by CODEOWNER.* (S14, §7.2)
- **SEC-041** `(v1-blocker)` NIF/NIE columns stored **column-encrypted** (libsodium `secretbox` via a small Rust wrapper; key in secrets). Application reads/writes through a typed wrapper that enforces encryption on write and allows decryption only at designated call sites. *Test: raw SQL SELECT on the column returns ciphertext.* (S17)
- **SEC-042** `(v1-blocker)` TOTP seeds column-encrypted as above. *Test: same.* (S6, S17)
- **SEC-043** `(v1-blocker)` Postgres user_id in `audit_log` and `calculations` is pseudonymized to the tombstone UUID on user erasure (ADR-005). *Test: erasure integration test confirms no FK to `users` row remains.* (S57)

### Logging rules (critical)

- **SEC-050** `(v1-blocker)` **Logging field allowlist, enforced at compile time.** `tracing::{info,warn,error,debug}` macros wrapped in a crate-level custom variant (`orbit_log::event!`) that accepts only whitelisted field types (IDs as `Uuid`, feature names as `&'static str`, booleans, integer counters, ISO timestamps). Attempting to log `Money`, `Grant`, `Scenario`, `Calculation`, `SellNowInput`, `Export`, `&str` matching NIF/NIE regex is a compile error (via a `#[deny_field_type]` proc-macro or equivalent). *Test: a fixture PR attempting `event!("grant_created", grant = ?grant)` fails to compile.* (S14, S21)
- **SEC-051** `(v1-blocker)` 4xx / 5xx API error bodies contain only an error code, a correlation ID, and a short localized message. **Never a stack trace, never a serialized request body, never an echo of user input.** *Test: fuzz harness submits grant-bearing payloads; response bodies scanned for numeric patterns matching share counts ⇒ must fail zero times.* (S14)
- **SEC-052** `(v1-blocker)` Product analytics event schema is allowlist-only per §7.2: event name + non-PII dimensions. Schema defined in code; emit site is a typed builder. *Test: analytics-event test harness rejects a prototype with grant_value dimension.* (S15)
- **SEC-053** `(v1-paid)` No third-party crash-reporting SaaS in v1. If Sentry/similar added v1.1+: EU-hosted project; `beforeSend` hook scrubs all FP/RC fields; configured before rollout. (S16)
- **SEC-054** `(v1-blocker)` **IP addresses logged only as `ip_hash`** — HMAC-SHA256 with a 32-byte salt in secrets, rotated annually (old salt retained 90 days for operator pivot). Raw IPs appear only in Caddy's transient access log (retained 7 days) and the Hetzner Cloud dashboard. *Test: `audit_log.ip_hash` matches the format; no `ip` field exists.* (S14)
- **SEC-055** `(v1-blocker)` Request-correlation IDs: every log line carries a `request_id` (UUIDv4 per request) and — where applicable — a `traceability_id`. No PII in either. *Test: log lines parseable to the schema.* (§7.9)

---

## 5. Encryption

- **SEC-060** `(v1-blocker)` **TLS 1.3 floor, TLS 1.2 rejected** at Caddy (`tls { protocols tls1.3 }` equivalent; disable TLS 1.2 and below). TLS 1.2 briefly permitted for ACME renewal only (Let's Encrypt supports TLS 1.3). *Test: `testssl.sh` or `nmap --script ssl-enum-ciphers` shows TLS 1.2/1.1/1.0 disabled on `app.orbit.<tld>`.* (§7.2)
- **SEC-061** `(v1-blocker)` **HSTS** on all app domains: `Strict-Transport-Security: max-age=15552000; includeSubDomains` from day one. **Add `preload` and submit to the HSTS preload list only after 30 days of stable production operation on TLS 1.3.** *Test: response header present; `max-age` ≥ 6 months.* (§7.9)
- **SEC-062** `(v1-blocker)` Cipher policy: only modern AEAD suites (TLS 1.3 defaults — `TLS_AES_256_GCM_SHA384`, `TLS_CHACHA20_POLY1305_SHA256`, `TLS_AES_128_GCM_SHA256`). No RC4, no 3DES, no CBC, no MD5. *Test: cipher list verified by `testssl.sh`.* (§7.2)
- **SEC-063** `(v1-blocker)` **At-rest encryption**: Hetzner volume encryption enabled on VM-1 and VM-2 data volumes. Postgres data directory on the encrypted volume. Hetzner Object Storage uses SSE-S3 by default. *Test: Hetzner Cloud console confirms encryption per volume.* (§7.2)
- **SEC-064** `(v1-blocker)` **Application-level encryption** for Financial-Personal classified columns flagged in §4 (NIF/NIE, TOTP seed). See SEC-041, SEC-042. (S17)
- **SEC-065** `(v1-blocker)` **Backup encryption**: nightly `pg_basebackup` + WAL stream tarred and encrypted with `age` (or libsodium `sealedbox`) using a key held in the secrets manager (NOT on the Storage Box). Restore runbook exercised quarterly. *Test: restore drill succeeds and is dated.* (S17)
- **SEC-066** `(v1-blocker)` Internal traffic between VM-1 (API/worker) and VM-2 (Postgres) over Hetzner Cloud's private network; Postgres bound to the private interface only, never public. **TLS required on the Postgres connection** (`sslmode=verify-full`) — the private network is trusted but belt-and-braces for v1 and mandatory once operator ≥ 2. *Test: `pg_hba.conf` rejects non-TLS; `netstat` shows Postgres listening only on the private IP.* (ADR-002)
- **SEC-067** `(v1-paid)` mTLS between API and worker processes is NOT required v1 (same host); MANDATORY if worker moves to a separate host in v1.1+. *Conditional on deployment topology change.* (S11)
- **SEC-068** `(v1-blocker)` Key-management: TOTP, backup, column-encryption keys are distinct, named, rotatable. Each key has a documented rotation procedure. (SEC-032, SEC-035)

---

## 6. Rule-set integrity (Orbit-specific, first-class)

Orbit's regulatory defensibility rests on rule-set integrity. This section is non-negotiable.

- **SEC-080** `(v1-blocker)` Rule-set YAML files under `/rules/<jurisdiction>/<id>.yaml` covered by CODEOWNERS requiring at least one CODEOWNER review on every PR touching `/rules/**`. Bot auto-merge forbidden on this path. *Test: GitHub branch protection verified; PR bypass attempt fails.* (S23, S50)
- **SEC-081** `(v1-blocker)` Canonicalization spec implemented as a library function used by both the CI hasher and the runtime loader; **SHA-256 content hash** is computed by Orbit at ingest time, never trusted from the YAML. Hash mismatch between runtime-recomputed hash and `rule_sets.content_hash` causes the engine to refuse to compute. *Test: tampered DB row causes deliberate engine refusal in integration test.* (S22, S24)
- **SEC-082** `(v1-blocker)` **Postgres row-level trigger**: `UPDATE` on `rule_sets WHERE status='active'` is rejected. Only `INSERT` (successor) and `UPDATE ... SET status='superseded'` on the predecessor are allowed. Migration that would mutate an active rule set fails the build. *Test: attempting `UPDATE rule_sets SET data = ... WHERE status='active'` raises.* (S22, S53)
- **SEC-083** `(v1-blocker)` Two-step publish: (a) PR merged on `main` ingests the YAML into `rule_sets` with `status='proposed'`; (b) promotion to `active` is a separate operator CLI command that performs the state-machine transition and creates an `audit_log` entry. Promotion requires Ivan's signed-in session (MFA-gated operator console). *Test: CLI refuses promotion if the rule set has a failing regression fixture.* (S23)
- **SEC-084** `(v1-blocker)` Regression test suite for rule sets includes **snapshot fixtures** of known-taxpayer calculations (synthetic personas with known totals for 2025 and 2026); any rate change that moves a fixture total triggers a deliberate test failure requiring acknowledgement in the PR description. *Test: rate-nudge PR fails CI.* (AC-4)
- **SEC-085** `(v1-blocker)` Engine determinism: (a) lint forbids `std::collections::HashMap` in calculation crates (use `BTreeMap`); (b) `LC_ALL=C` set in worker; (c) `now()` is injected into the engine, never read from the system clock inside a calculation; (d) periodic replay sampler (ADR-004) re-runs a weekly random sample of old calcs and alerts on hash mismatch. *Test: lint blocks `HashMap` in `orbit-tax-*` crates.* (S25)
- **SEC-086** `(v1-blocker)` Every calculation persists `rule_set_id`, `rule_set_content_hash`, `engine_version`, `inputs_hash`, `result_hash` per ADR-004 §Calculation stamping. *Test: calculation row has all five non-null.* (§7.1)
- **SEC-087** `(v1-blocker)` `proposed` rule sets are NOT usable for user-visible calculations; loader filter is `status='active'` for user paths. Internal-test path is gated by a build feature flag disabled in production. *Test: request to `/scenario/run` with a forced `proposed` rule set returns 500 with a specific error code in non-prod and is impossible to trigger in prod.* (S26)

---

## 7. Export artefact integrity

- **SEC-090** `(v1-blocker)` Every export (PDF and CSV) includes the visible footer specified in ADR-008 §"Visible footer" on **every page** (PDF) and in the comment block (CSV), containing: traceability ID, rule set ID, AEAT guidance date, engine version, inputs hash, result hash, computed-at, ECB-FX-as-of, and the ES + EN disclaimer. *Test: golden-file render of a fixture calculation matches the expected footer on pages 1 and last.* (ADR-008)
- **SEC-091** `(v1-blocker)` PDF XMP metadata carries `orbit:traceability_id`, `orbit:rule_set_id`, `orbit:rule_set_content_hash`, `orbit:inputs_hash`, `orbit:result_hash`, `orbit:computed_at`, `orbit:engine_version`. **PDF metadata does NOT carry the input values themselves** (share counts, prices, tax totals) — only hashes and IDs. *Test: XMP extraction confirms the fields; input values absent.* (S18, ADR-008)
- **SEC-092** `(v1-blocker)` Auth-scoped verification endpoint `GET /verify/<traceability_id>` returns `{rule_set_id, rule_set_content_hash, inputs_hash, result_hash, computed_at, engine_version, superseded: bool}` **only to an authenticated user who owns the export**. Unauthenticated requests return 404. *Test: cross-user verify probe returns 404.* (S33)
- **SEC-093** `(v1-paid, v1.1-hardening)` Cryptographic PDF signing (PAdES detached, or embedded PGP) deferred to v1.1; v1 ships with hash-and-verify only. (S33)
- **SEC-094** `(v1-blocker)` CI reproducibility test: for each fixture calculation, generate an export, then re-run the engine against the stored inputs + pinned rule set and assert `new_result_hash == export.result_hash`. *Test: CI job green weekly; alert on mismatch.* (ADR-008 §Reproducibility test)
- **SEC-095** `(v1-blocker)` Export retention: `exports.retained_until = created_at + 7 years`; weekly worker sweep deletes objects past retention. On user erasure, export objects are deleted **immediately**, overriding the 7-year retention (ADR-008 + S36). *Test: erasure integration test confirms object deletion.* (S36, S57)

---

## 8. Audit logging

- **SEC-100** `(v1-blocker)` Every auth event (signin success/fail, signup, password change, MFA enable/disable, session revoke), every rule-set publish/promote, every export, every grant edit, every DSR action, every operator action produces an `audit_log` entry per the schema in ADR-005 §Audit & GDPR. *Test: exhaustive list of actions covered; a fixture run exercises one of each.* (§7.9)
- **SEC-101** `(v1-blocker)` `audit_log.payload_summary` is **typed and schema-validated** to the allowlist: action-specific non-FP dimensions only. Attempting to write a Money, share count, or tax total to payload fails at compile time (same mechanism as SEC-050). *Test: fixture PR attempting to log `grant_value` in payload fails.* (§7.2, S15)
- **SEC-102** `(v1-blocker)` `audit_log` is append-only: `orbit_app` role has `INSERT` but not `UPDATE` or `DELETE` on the table. Retention cleanup runs as a separate role with a time-bounded `DELETE` based on `occurred_at`. *Test: `UPDATE audit_log ...` by orbit_app fails with permission denied.* (§7.9)
- **SEC-103** `(v1-blocker)` Audit-log retention: **6 years** from `occurred_at` (AEAT prescription). Entries older than 6 years deleted by the weekly retention worker. *Test: retention worker deletes a synthetic 6y+1d entry.* (§7.9)
- **SEC-104** `(v1-blocker)` Audit-log access is restricted: read access is granted only to the `orbit_support` role (operator-only); queries against it are themselves logged to a `support_access_log` table, append-only. *Test: `orbit_app` role receives permission-denied on SELECT of `audit_log`.* (S52, S56)
- **SEC-105** `(v1-paid)` Monthly operator-access review: the operator reviews `support_access_log` for the prior month, signs off in the change-management log. *Test: sign-off artefact dated within 35 days of month-end.* (S56)
- **SEC-106** `(v1-blocker)` Traceability IDs on exports appear in the corresponding `audit_log.traceability_id` column for reverse-lookup. *Test: export-generation integration test confirms the link.* (ADR-008)

---

## 9. GDPR / LOPDGDD operational

- **SEC-120** `(v1-blocker)` **Privacy policy + cookie notice** published pre-launch (ES + EN), listing: identity of the controller, purposes of processing, legal bases, retention periods, sub-processors, DSR procedures, contact (privacy@... + postal address), supervisory authority (AEPD). *Test: documents reviewed by legal; linked from every page footer.* (S61)
- **SEC-121** `(v1-blocker)` **Sub-processor register** published and updated on change: Hetzner (hosting, EEA), Bunny.net (CDN, EU PoPs verified), ECB (public feed — not a processor, documented for transparency), Finnhub (market data, no PII transmitted), Postmark or Scaleway Transactional Email (transactional email), Stripe or Paddle (billing — the exact choice per OQ-03). Each row: role, data categories, region, legal transfer mechanism (SCCs + TIA where non-EEA). *Test: register linked from privacy policy.* (S59)
- **SEC-122** `(v1-blocker)` **DPA template** for paid users, auto-accepted at paid-tier checkout; retained with subscription record. Free-tier users receive the privacy notice; a DPA is offered on request. (§7.2)
- **SEC-123** `(v1-blocker)` **DSR self-service** per US-011: access/portability export delivered within 7 days (target) / 30 days (hard SLA per Art. 12(3)); rectification via profile edit with audit entry; erasure via two-step confirm with 30-day soft-delete grace then hard-delete cascade; restriction via a flag that suspends calc/export while retaining data. *Test: for each DSR kind, end-to-end integration test covers the flow and asserts SLA timer.* (S57, S58)
- **SEC-124** `(v1-blocker)` Erasure semantics documented in the privacy policy: what is deleted (all `[RLS]` data, exports in object storage, email-provider bounce logs on next rotation), what is retained under legal obligation (pseudonymized `calculations` + `audit_log` for 6 years — AEAT prescription, disclosed). *Test: privacy policy text reviewed.* (S57, S63)
- **SEC-125** `(v1-blocker)` **Breach-notification runbook**: 72h timer from "aware of personal-data breach" to AEPD notification; templated initial + follow-up notifications (ES + EN); user notification template per Art. 34. Runbook exercised at least once before paid launch. *Test: runbook drill artefact dated.* (S60)
- **SEC-126** `(v1-blocker)` Privacy contact published (email + postal address for written DSARs). DPO appointment deferred pending OQ-02 re-evaluation at 10k MAU. (S61)
- **SEC-127** `(v1-blocker)` Cookie-consent banner: if non-essential cookies are ever introduced, AEPD 2023-compliant (Aceptar / Rechazar / Configurar at equal prominence, no pre-ticked, consent logged). v1 ships with essential-only cookies (session, CSRF) so the banner is informational; no consent required. *Test: first load sets only essential cookies.* (S62)

---

## 10. Third-party & supply chain

- **SEC-140** `(v1-blocker)` `cargo audit` and `cargo deny` in CI on every PR and on a nightly scheduled run on `main`. `deny` config covers: known-malicious crates (RUSTSEC advisories), unmaintained crates, duplicate dependencies, disallowed licenses. *Test: audit job is a required check on `main`.* (S47)
- **SEC-141** `(v1-blocker)` `npm audit` (or `pnpm audit`) in CI on every PR and nightly. High-severity findings fail the build. *Test: same.* (S47)
- **SEC-142** `(v1-blocker)` **CycloneDX SBOM** generated per release (Rust + npm combined); attached to the release artefact; retained with the release. *Test: release artefact contains `sbom.cdx.json`.* (S47)
- **SEC-143** `(v1-blocker)` **Dependency review cadence**: weekly auto-PR via Dependabot or Renovate for patch-level updates; bi-weekly grooming of minor/major updates. No auto-merge; Ivan reviews. (S47)
- **SEC-144** `(v1-blocker)` **GitHub Actions pinned to commit SHA** (no floating `@v3`). Third-party Actions reviewed before first use. Action permissions scoped via `permissions:` block per job; default minimum. *Test: a PR introducing `actions/checkout@v4` without a SHA fails a dedicated workflow lint.* (S48)
- **SEC-145** `(v1-blocker)` CI secrets live in GitHub **Environments** (`production` with required reviewers). Build-only vs deploy-only jobs separated; build jobs never see deploy secrets. *Test: PR from a fork cannot obtain a prod secret.* (S49)
- **SEC-146** `(v1-blocker)` Docker base images pinned by digest (`@sha256:...`), preferring `cgr.dev/chainguard/static` or `gcr.io/distroless/cc` for Rust runtimes. Trivy scan on built images; high-severity findings block release. Weekly rebuild to absorb upstream patches. *Test: built image digest recorded in release notes.* (S51)
- **SEC-147** `(v1-blocker)` **Vendor security posture verified for market-data (ADR-006) and FX (ADR-007)** vendors before first production request:
  - Finnhub ToS confirmed to permit SaaS redistribution of delayed quotes — logged in `docs/security/vendor-licensing-finnhub.md` at contract time.
  - Outbound integration test asserts request body/headers carry ONLY API key + ticker — no user identifier, no grant value.
  - Vendor's privacy / security policy reviewed and filed. (S29, ADR-006 hand-off)
- **SEC-148** `(v1-blocker)` **ECB XML fetch** uses TLS cert-chain validation (no `--insecure`); response schema validated; `<Cube time>` must be today or within `MAX_FALLBACK_DAYS` past. *Test: mock server returning future date ⇒ fetch rejected.* (S28)
- **SEC-149** `(v1-blocker)` Outbound-connection allowlist at the host firewall (nftables on VM-1): only the vendor endpoints (Finnhub, ECB, email provider, billing provider, Let's Encrypt, Hetzner Object Storage, GitHub for deploy) reachable from the app. Default-deny outbound. *Test: `curl` to a random internet host from the app user is blocked.* (S49, S54)
- **SEC-150** `(v1-blocker)` **Gitleaks** pre-commit hook + CI scan on every push + full-history scan on every deploy. Findings block merge. *Test: commit containing `-----BEGIN PRIVATE KEY-----` fails pre-commit.* (S55)

---

## 11. Abuse prevention

- **SEC-160** `(v1-blocker)` **Rate limits** (per-user / per-IP):
  - `POST /auth/signin` — 10/IP/10m, 5/account/10m.
  - `POST /auth/signup` — 5/IP/hour.
  - `POST /auth/reset` — 3/email/hour, 5/IP/hour.
  - `POST /grants` (CSV import) — 10/user/hour.
  - `POST /scenario/run` — 60/user/min, 600/user/hour.
  - `POST /sell-now/compute` — 60/user/min, 600/user/hour.
  - `POST /exports` — 20/user/hour, 100/user/day.
  - `POST /dsr/*` — 1/user/day except `dsr/rectification` which is 5/day.
  - `GET /market/quote/:ticker` — server-side only (coalesced), no direct public; 4/ticker/hour vendor-facing.
  - Rate-limit store: Postgres-backed (leaky-bucket table) to avoid a Redis dependency (ADR-001). (S41, S43, S44, S45)
- **SEC-161** `(v1-blocker)` **CAPTCHA** (hCaptcha EU endpoint or Turnstile) on signup and on signin after 3 consecutive failures for an account. (S1, AC-8)
- **SEC-162** `(v1-blocker)` Email verification required before any grant is saved (confirms address ownership). (AC-8)
- **SEC-163** `(v1-blocker)` **Input validation**:
  - Typed DTOs with `serde` + `validator` at every request boundary.
  - Length caps: `grants.notes` ≤2,048; `grants.employer_name` ≤256; `grants.ticker` matches `^[A-Z0-9.\-]{1,8}$`; `scenarios.name` ≤128; Art. 7.p free fields ≤512.
  - Numeric bounds on all money fields (reject negative shares, negative prices, strike ≥ current price on NSO same-day).
  - CSV import: ≤1,000 rows + ≤5 MB (OQ-04); row-level validation; malformed rows reported back per US-002. (S41, S42)
- **SEC-164** `(v1-blocker)` **Market-data sanity check**: reject a quote whose absolute price change vs most recent cached prior exceeds 30% (configurable). On reject, surface "vendor unavailable" state. *Test: mock vendor returning 10× price ⇒ rejected.* (S27)
- **SEC-165** `(v1-blocker)` **Compute budget**: sell-now and scenario computations hard-capped at 500ms wall-clock on the request thread (§7.8); timeout returns 503 with a generic message. *Test: deliberately-large fixture returns 503.* (S41)
- **SEC-166** `(v1-blocker)` **Bot protection on the verify endpoint**: `GET /verify/<traceability_id>` behind auth and per-user rate-limited (60/user/min). (S33, S35)

---

## 12. Headers, CSP, CORS baseline

All values are **opinionated concrete defaults**. Tighten at go-live, don't loosen.

- **SEC-180** `(v1-blocker)` **Content-Security-Policy** (on the SPA HTML shell and on every API response that returns HTML):
  ```
  Content-Security-Policy:
    default-src 'self';
    script-src 'self';
    style-src 'self';
    img-src 'self' data:;
    font-src 'self';
    connect-src 'self' https://api.orbit.<tld>;
    frame-ancestors 'none';
    form-action 'self';
    base-uri 'self';
    object-src 'none';
    upgrade-insecure-requests
  ```
  No `'unsafe-inline'`, no `'unsafe-eval'`. If a third-party service needs to be allowed (e.g., Stripe.js on the billing page), scope it to that route's CSP only. *Test: response header present on `/app/index.html`; inline `<script>alert(1)</script>` does not execute in a fixture.* (S3, S38)
- **SEC-181** `(v1-blocker)` **Strict-Transport-Security**: `max-age=15552000; includeSubDomains` v1; `preload` added after 30-day stability. (SEC-061)
- **SEC-182** `(v1-blocker)` **X-Content-Type-Options**: `nosniff`. (Defence against MIME-sniff.)
- **SEC-183** `(v1-blocker)` **X-Frame-Options**: `DENY` (superseded by CSP `frame-ancestors 'none'` but cheap belt-and-braces for old UAs).
- **SEC-184** `(v1-blocker)` **Referrer-Policy**: `strict-origin-when-cross-origin`.
- **SEC-185** `(v1-blocker)` **Permissions-Policy**: `geolocation=(), camera=(), microphone=(), payment=(self), usb=()` — at minimum deny the unused APIs.
- **SEC-186** `(v1-blocker)` **Cross-Origin-Opener-Policy**: `same-origin`; **Cross-Origin-Embedder-Policy**: `require-corp` where feasible (v1 SPA should tolerate it).
- **SEC-187** `(v1-blocker)` **CORS**: the API's `Access-Control-Allow-Origin` is EXACTLY `https://app.orbit.<tld>` (explicit origin, not `*`, not wildcard); `Allow-Credentials: true`; `Allow-Methods` restricted per-route; `Allow-Headers` whitelisted (`Content-Type`, `Authorization`, custom `X-Request-ID`). Preflight cache `Access-Control-Max-Age: 600`. *Test: cross-origin request from `https://evil.example` is blocked by CORS.* (S3)
- **SEC-188** `(v1-blocker)` **CSRF protection**: since cookies are `SameSite=Lax`, the `POST` baseline is safe from classic CSRF, but we add a **double-submit token** (CSRF token in a response cookie, echoed in a request header by the SPA) on all state-changing endpoints. *Test: POST without the header returns 403.* (S3)
- **SEC-189** `(v1-blocker)` **Cookie flags**: all auth-carrying cookies `HttpOnly; Secure; SameSite=Lax; Path=/`. Consider `SameSite=Strict` on the refresh cookie if UX allows. (SEC-006)
- **SEC-190** `(v1-blocker)` **CSP reporting endpoint** (`report-to` / `report-uri`) accepts reports and stores them in a rotating log for operator review (not in `audit_log`). Use this to detect XSS attempts and CSP drift. (S3)

---

## 13. Cross-cutting / governance

- **SEC-200** `(v1-blocker)` **Security review gate**: every PR touching auth, crypto, RLS, rule-sets, exports, or third-party integrations requires the `security-review` label and an approving review from the security reviewer. (§7.9, governance)
- **SEC-201** `(v1-blocker)` **Product-copy review** on every PR touching user-facing text: no "should", "recomiendo", "mejor", "debería" etc. that crosses into advice. (S38, R-1)
- **SEC-202** `(v1-paid)` **Third-party pen-test** before paid-tier launch (§7.9). Scope must cover: auth, authz/RLS, CNMV/advice-positioning UX copy review, rule-set pipeline, export integrity, data-plane boundaries. Findings triaged; Blocker/Major closed before paid launch.
- **SEC-203** `(v1-blocker)` **Incident response runbook**: detection sources, triage, escalation to operator, AEPD 72h timer, user-notification templates, post-mortem template. Drilled once before paid launch. (S60)
- **SEC-204** `(v1-blocker)` **Restore drill**: at least one full Postgres restore from `pg_basebackup` + WAL exercised before paid launch, documented and dated. (ADR-002 follow-up)
- **SEC-205** `(v1-blocker)` **Operator MFA**: MFA mandatory on every operator-facing account (GitHub, Hetzner Cloud, domain registrar, DNS host, Bunny.net, billing provider, email provider). Hardware security key preferred on at least one. (S54)

---

## 14. Conditional requirements (flagged for solution-architect)

These fire only if architectural choices currently open are made in a particular direction. Called out so the architect can close them on the second-pass ADR.

- **SEC-300** `(conditional)` **If** the worker process moves to a separate host (v1.1+), **then** mTLS is required on API↔worker and the worker's Postgres credentials are scoped to only the tables it needs. (S11, S67)
- **SEC-301** `(conditional)` **If** a third-party crash-reporting or APM SaaS is introduced, **then** EU-hosted, PII scrubber configured, added to sub-processor register. (SEC-053)
- **SEC-302** `(conditional)` **If** any LLM / AI feature is introduced, **then** re-run this threat model with prompt-injection, output-leak, and training-data-exfiltration categories added; isolate user text via a strict input/output sanitizer. (S37)
- **SEC-303** `(conditional)` **If** the billing provider stores/returns data beyond customer ID + status (e.g., card details), **then** PCI-DSS scoping review required; prefer a redirect / hosted-fields integration to keep Orbit out of PCI scope. (ADR hand-off OQ-03)
- **SEC-304** `(conditional)` **If** a public (unauthenticated) export-verification endpoint is introduced in v1.1, **then** rate-limit per traceability ID (e.g., 10/hour) and return a redacted subset (no inputs hash, only status: "valid/expired/superseded"). (S33, S35)
- **SEC-305** `(conditional)` **If** CSV exports are later signed (PAdES-equivalent for CSV doesn't exist — consider detached PGP or X.509 over the comment-block hash), **then** key-management story required. (S33 v1.1)

---

*End of security requirements.*
