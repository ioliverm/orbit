# ADR-002: Cloud provider and deployment topology

- **Status:** Proposed
- **Date:** 2026-04-18
- **Deciders:** Ivan (owner)

## Context

Orbit v1 has a hard cost ceiling of **<€200/mo for all infrastructure** while validating product-market fit. EU-only hosting is a non-negotiable GDPR/LOPDGDD requirement (§7.2). The application is a Rust binary + Postgres + small object storage + a static SPA bundle behind a CDN. Traffic during validation is realistically single-digit concurrent users; load matters far less than fixed monthly minimums and per-component pricing minimums.

This budget excludes the AWS / GCP / Azure managed-services tier (their managed Postgres alone — RDS `db.t4g.small` Multi-AZ, Cloud SQL HA, Azure Flexible Server — starts north of €80–150/mo before storage, plus another €60–100/mo for Fargate/Run/Container Apps, plus NAT, plus egress). Realistic EU-native, Rust-friendly, Postgres-friendly options at this price are **Hetzner Cloud** and **Scaleway**, with OVHcloud as a distant third.

## Decision

**Provider: Hetzner Cloud (Falkenstein region, Germany — fully inside EEA, AEPD-friendly jurisdiction).**

**Topology (v1):**

| Component | Where it runs | SKU | ~Monthly EUR (excl. VAT) |
|---|---|---|---|
| API process (`orbit api`) | VM #1 | Hetzner CX22 (2 vCPU, 4 GB RAM, 40 GB NVMe) | €4.51 |
| Worker process (`orbit worker`) | VM #1, same host, separate systemd unit | (shared) | (included) |
| Reverse proxy + TLS termination | VM #1, Caddy | (shared) | (included) |
| **Postgres 16 (self-managed)** | VM #2, dedicated | Hetzner CX32 (4 vCPU, 8 GB RAM, 80 GB NVMe) | €7.55 |
| Postgres backups | Hetzner Storage Box BX11 (1 TB), nightly `pg_basebackup` + WAL archive | BX11 | €4.27 |
| Object storage (DSR exports, PDF worksheets, market-data history snapshots) | Hetzner Object Storage (S3-compatible, EU) | Pay-per-use, ~50 GB v1 | ~€2 |
| SPA static assets + edge cache | **Bunny.net CDN** (EU-only PoPs configurable; cheap; GDPR-compliant) | Volume tier | ~€1 (negligible at v1 traffic) |
| Outbound email (transactional, DSR notices) | Scaleway Transactional Email or Postmark EU | Free tier or ~€10/mo | ~€10 |
| Monitoring (uptime + log aggregation) | Better Stack (free tier) or self-hosted Uptime Kuma on VM #1 | Free → €10 | ~€0–10 |
| Snapshots (volume snapshots of both VMs, weekly) | Hetzner | ~€1/VM/mo | ~€2 |
| **Subtotal** | | | **~€31–41/mo** |
| Buffer for traffic / market-data vendor (ADR-006) / billing-provider fees | | | ~€30 |
| **Total estimate** | | | **~€60–70/mo** — well under €200/mo ceiling |

This leaves substantial headroom for: vertical-scaling the Postgres VM if needed, paying for a managed Postgres later, or absorbing market-data vendor cost.

**Region:** Falkenstein (FSN1) primary; Helsinki (HEL1) is a future DR option. Both fully EEA. No data leaves EEA in v1.

**Deployment mechanism:** Single binary built in CI, shipped as a Docker image to a private Hetzner registry (or just `scp` + systemd for v1 simplicity). Deploys via `systemctl restart`. Blue/green not yet justified at this scale — short downtime windows are acceptable; user-visible impact is low.

**TLS:** Caddy with automatic Let's Encrypt. Two domains: `app.orbit.<tld>` (SPA + API) and `orbit.<tld>` (marketing static site, also via Bunny.net CDN).

**Backups & DR (v1):**
- Postgres: nightly `pg_basebackup` + continuous WAL archive to Hetzner Storage Box. RPO ~5 min, RTO ~1 h via documented runbook (manual restore acceptable at this scale).
- Object storage: Hetzner Object Storage has built-in replication.
- VM-level: weekly automated snapshots.
- A documented restore drill is a launch-blocker (see follow-up).

## Alternatives considered

- **Scaleway (Paris/Amsterdam).** Real candidate. DEV1-S instance ~€8.99/mo; managed Postgres "Database for PostgreSQL" starts ~€19/mo for a 2 GB instance (cheaper than AWS RDS but pricier than self-managing on Hetzner). EU-native, fully GDPR-aligned. **Rejected primarily on price-per-vCPU** (Hetzner is roughly 2× cheaper for equivalent compute) and on Hetzner's stronger reputation for sustained CPU performance. Scaleway remains a viable migration target if Hetzner has an outage/incident pattern.
- **OVHcloud.** Cheapest of the three on paper; rejected on operational reputation — historical incidents (Strasbourg fire 2021), more variable support, dashboard ergonomics weaker. Acceptable as a third option if both above fail.
- **Hetzner managed Postgres.** Hetzner does not (as of training data) offer first-party managed Postgres. **Verify before launch** — if they have launched one in the EU at competitive pricing, switching to managed is a low-risk follow-up that trades €15–25/mo for removing an ops burden.
- **Neon EU / Supabase EU as managed Postgres.** Neon EU free tier exists but cold-start latency on the free tier is incompatible with the 500 ms P95 NFR; paid tier starts ~€19/mo and adds a non-EEA-headquartered processor (US company, EU region) which is acceptable under SCCs but adds DPA paperwork. Self-managed on Hetzner is simpler v1.
- **Fly.io.** Has EU regions, attractive ergonomics, but Postgres on Fly is community-supported, billing in USD, and the company's cost trajectory is uncertain. Rejected for v1.
- **Kubernetes (any flavour).** Vastly over-engineered for one binary + one Postgres. Explicitly rejected.

## Consequences

**Positive:**
- ~€60–70/mo all-in is comfortably under the €200/mo ceiling, with ~3× headroom for vendor/traffic surprises.
- Hetzner is EEA-domiciled, EU-only data plane, no US sub-processor exposure for compute or storage.
- Single-region two-VM topology is genuinely simple to reason about and document in the security review.
- Same SKUs scale up ~10× before topology needs to change (CX22 → CCX23/33; Postgres VM → CCX or dedicated).

**Negative / risks:**
- **Self-managed Postgres is the load-bearing tradeoff.** Ivan owns: PG version upgrades, WAL archiving health, restore drills, vacuum/autovacuum tuning, `pg_dump` for DSR exports. Mitigation: standard Postgres operational playbook, weekly snapshots, monthly restore drill. If this becomes painful, switching to a managed provider is reversible in <1 day.
- Single-region, single-AZ-equivalent. A Hetzner Falkenstein outage takes Orbit down. Acceptable for the 99.5% v1 SLO (§7.8); revisit at paid-tier scale.
- Two-VM single-binary topology means a noisy worker (e.g., DSR export of a huge account) could compete with the API for CPU. Mitigated by the dedicated worker process having its own systemd resource limits (`CPUQuota`).
- Bunny.net is a Slovenian company; verify EU-only PoP configuration is enforced and documented for the security review.
- Self-managed Postgres on a single VM means **no Multi-AZ HA**. If the Postgres VM dies, recovery is restore-from-backup — minutes-to-an-hour of downtime. Documented and accepted at v1 SLO.

**Follow-ups:**
- Verify Hetzner managed Postgres status at launch; if priced <€25/mo for adequate spec, switch.
- Verify Bunny.net EU-only PoP config and add to security-engineer review pack.
- Document Postgres restore runbook and execute one drill before paid-tier launch (launch-blocker).
- Verify outbound-email provider's EU-data-residency and DPA before launch; Postmark is US-domiciled (acceptable with SCCs but document it).
- Decide whether the marketing site uses Astro or hand-rolled static HTML — implementation-pass detail, not architectural.
