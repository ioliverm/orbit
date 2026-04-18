# GitHub repository setup — Slice 0a

- **Owner:** product owner (ioliverm)
- **Scope:** apply the GitHub-side settings that back security-checklist
  items S0-05, S0-06 (partial — 0a share), S0-07. The CI workflows that
  depend on these settings already live under `.github/workflows/` and
  will be rejected by branch protection only once this runbook has been
  applied.
- **Companions:**
  - `docs/security/security-checklist-slice-0.md` — item text.
  - `docs/adr/ADR-015-slice-0-local-first-scope-split.md` — 0a/0b scope.
  - `docs/adr/ADR-013-repository-and-deployment-scaffold.md` §CI, §Deployment.

This runbook is **PO-applied**. Nothing in this file can be automated by
the repo code itself; it documents exact settings the owner must tick in
GitHub's UI (or invoke via `gh api`, which is faster and auditable).

> This runbook was authored in a sandbox with no `gh` authentication; **no
> API calls were issued**. Every step below is left for the PO to apply
> manually or via the ready-to-run `gh api` snippets provided under each
> step. Once applied, tick the "done / date / applied-by" checkboxes at
> the end.

---

## 1. S0-05 — Default `GITHUB_TOKEN` permissions + workflow approvals

### 1.1 Set default token permissions to read

**UI path:** Repo -> Settings -> Actions -> General -> "Workflow
permissions" -> select **"Read repository contents and packages
permissions"** (NOT "Read and write"). Save.

This makes the repo default match the per-job `permissions:` blocks we
set in every workflow (defense in depth — if a job forgets to declare
`permissions:`, it still gets read-only).

**`gh api` equivalent:**

```sh
gh api -X PUT repos/ioliverm/orbit/actions/permissions/workflow \
  -F default_workflow_permissions=read \
  -F can_approve_pull_request_reviews=false
```

### 1.2 Require approval for outside-collaborator workflows

**UI path:** Repo -> Settings -> Actions -> General -> "Fork pull request
workflows from outside collaborators" -> select **"Require approval for
first-time contributors who are new to GitHub"** (or stricter:
**"Require approval for all outside collaborators"** while Orbit is
solo-owner). Save.

There is no stable `gh api` endpoint for this toggle; apply in the UI.

### 1.3 Require code-owner review on workflow changes

This is enforced by the branch-protection rule (Section 3) plus the
`CODEOWNERS` file (already committed, covers `.github/workflows/`). No
extra toggle is needed here once Section 3 is applied.

---

## 2. S0-06 (0a share) — `production` Environment with required reviewer

Slice 0a commits `deploy.yaml` with every job guarded by `if: false`
(see `docs/adr/ADR-015-slice-0-local-first-scope-split.md`). The
environment must nonetheless exist now, so the reference in the
workflow resolves and so Slice 0b activates the deploy by flipping
a single guard rather than setting up GitHub chrome.

### 2.1 Create the `production` environment

**UI path:** Repo -> Settings -> Environments -> **New environment** ->
name: `production`.

**`gh api` equivalent (creates or updates):**

```sh
gh api -X PUT repos/ioliverm/orbit/environments/production \
  -F wait_timer=0 \
  -F 'deployment_branch_policy=null' \
  -F 'reviewers[][type]=User' \
  -F 'reviewers[][id]='$(gh api users/ioliverm --jq .id)
```

The reviewer is **ioliverm** (sole operator). Wait timer is `0` —
manual review is the gate, not a time delay.

### 2.2 Confirm the environment is bound to `deploy.yaml`

`deploy.yaml` already references `environment: production` on the
`migrate` and `deploy` jobs. No extra wiring needed; verify after
creation by loading `Settings -> Environments -> production` and
checking that "Deployments" will list runs once the workflow activates
(it will not until 0b flips `if: false`).

### 2.3 Deferred to 0b

- Real secrets (SSH deploy key, Postmark, Stripe, Finnhub, etc.)
  bound to the `production` environment. Slice 0a adds no production
  secrets.
- Activation of `deploy.yaml`: remove the `if: false` guard and the
  `# 0b gate:` comment on each job. Do this in the PR that closes 0b.

---

## 3. S0-07 — Branch protection on `main`

### 3.1 Required reviews

- Require a pull request before merging.
- Require at least **1** approving review.
- **Require review from Code Owners** (this ties into `CODEOWNERS`).
- Dismiss stale approvals when new commits are pushed.
- Require approval of the most recent reviewable push.

### 3.2 Required status checks

The following **must pass** before merge. Names are the `name:` of each
job in `.github/workflows/*.yaml` — GitHub lists them once each has run
at least once on a PR.

From `ci.yaml`:

- `backend (fmt, clippy, test, check)`
- `backend-audit (cargo-audit, cargo-deny)`
- `frontend (install, lint, typecheck, test, audit)`
- `xtask-check (S0-08 custom lints)`
- `sbom (cyclonedx cargo + npm)`

From `pre-merge-gitleaks.yaml`:

- `gitleaks (secret scan)`

Also tick **"Require branches to be up to date before merging"** so
required checks rerun on rebases.

### 3.3 Additional toggles

- **Require linear history** — recommended (ADR-013 prefers rebase-or-squash).
- **Require conversation resolution before merging** — recommended.
- **Do not allow bypassing the above settings** — tick this; there is no
  emergency-bypass process at Slice 0a.
- **Restrict who can push to matching branches** — leave empty; the PR
  requirement is the gate.
- **Allow force pushes** — off. **Allow deletions** — off.

### 3.4 `gh api` equivalent (one shot)

```sh
gh api -X PUT repos/ioliverm/orbit/branches/main/protection \
  --input - <<'JSON'
{
  "required_status_checks": {
    "strict": true,
    "contexts": [
      "backend (fmt, clippy, test, check)",
      "backend-audit (cargo-audit, cargo-deny)",
      "frontend (install, lint, typecheck, test, audit)",
      "xtask-check (S0-08 custom lints)",
      "sbom (cyclonedx cargo + npm)",
      "gitleaks (secret scan)"
    ]
  },
  "enforce_admins": true,
  "required_pull_request_reviews": {
    "dismiss_stale_reviews": true,
    "require_code_owner_reviews": true,
    "required_approving_review_count": 1,
    "require_last_push_approval": true
  },
  "required_linear_history": true,
  "allow_force_pushes": false,
  "allow_deletions": false,
  "required_conversation_resolution": true,
  "restrictions": null
}
JSON
```

Re-run any time the status-check set changes (for example when T5 lands
and `xtask-check` flips from warn-only to enforcing).

### 3.5 CODEOWNERS sanity check

`.github/CODEOWNERS` (already committed) must cover these paths:

- `/rules/**`
- `/migrations/**`
- `.github/workflows/**`
- `backend/crates/orbit-auth/**`
- `backend/crates/orbit-crypto/**`

If any is missing, add it before applying Section 3 — the "require code
owners" rule silently passes if no code owner matches a path.

---

## 4. What was applied in this runbook's commit

The authoring agent had no `gh` authentication in its sandbox. **No
`gh api` calls were issued by the repo change that shipped this file.**

The PO must work through Sections 1–3 and tick the boxes below as each
is applied. Section 4 is the place to record any deviation from this
runbook (for example, a different reviewer set or a different required-
check list).

### Apply log

- [ ] 1.1 Default token permissions set to read — applied by _____ on _____
- [ ] 1.2 Outside-collaborator workflow approvals — applied by _____ on _____
- [ ] 2.1 `production` environment + reviewer (ioliverm) — applied by _____ on _____
- [ ] 3.x Branch protection on `main` — applied by _____ on _____
- [ ] CODEOWNERS confirmed on all required paths — verified by _____ on _____

---

## 5. Deviations

_Record any deviation from the defaults above here, with justification
and date. Empty at sign-off is ideal._

- _(none)_
