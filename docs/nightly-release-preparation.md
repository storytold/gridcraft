# Windows, Linux and macOS release-ref nightly: supervised manual go-live

**Enabled for supervised manual runs, not yet qualified.** The producer (`nightly.yml`, main) and the
signer (`nightly-sign.yml`, release) keep only their exact context guards and run when one of the reviewed
manual actors dispatches them. There is no schedule yet. The installed policy
[`.github/nightly-signing-policy.json`](../.github/nightly-signing-policy.json) is the v3 policy
adapted from `storytold/photocraft#16594`. Once `nightly.yml` lands on `main`, populate
`producer_workflow_id` before promoting the policy to `release`, read with:

```bash
gh api repos/storytold/gridcraft/actions/workflows/nightly.yml --jq .id
```

The callers pass no platform secrets. The unsigned reusable caller's maximum OIDC/attestation
permissions are for its isolated attester; its compiler narrows to contents and Actions read
only, with no OIDC, attestation write, release environment or platform signing secrets.
Shared tooling is pinned to `605d58766e8f536564da8fc81d3676ac86816dea`, the craft-common-actions
main commit that merged [#339](https://github.com/storytold/craft-common-actions/pull/339) (bounded
lineage compares and an issue reread at the end of candidate admission) on top of
[#342](https://github.com/storytold/craft-common-actions/pull/342) (policy v3, the `platforms` input
and the Linux and macOS legs). Both callers name it in `uses` and `tooling-revision`. A pin is reviewed
source availability, not provider, queue, native signing or crypto qualification.
Existing stable workflows are unchanged.

**First runs.** The first runs are manual dispatches in this order: `nightly.yml` on main (only a
listed manual actor passes the producer's plan step), then `nightly-sign.yml` on `release` with that
producer run ID and attempt, by a listed manual actor.

**Manual actors.** `bflatastic`, `dylanmtaylor`, `echelon`, `FredApps` and `skylartaylor`, in the
producer's `manual-actors` and the policy's `manual_actors`. The list mirrors the release-managers
team (id 19837241), leaving out `echelon-robot`; update it by PR in both places when the team changes.

**Admission timing.** The candidate check reads the newest CI run on the candidate commit and
main's latest finished Sonar analysis, so when to dispatch matters:
- Pick a quiet main, wait for its CI and SonarCloud analysis to finish green, then dispatch the
  producer and, once it succeeds, the signer, before another main push lands a newer analysis.

**Platform rows.** The producer caller lists
`platforms: '["windows-x86_64","linux-x86_64","macos-aarch64"]'` on its single job: one producer
run builds every listed platform for one all-or-nothing release (one failed or skipped platform stops
that night's release), and the release-ref signer admits platforms only from the protected policy,
so there is no separate Linux or macOS caller job and the signer caller has no platform input.

## Installed policy values and their sources

Read on 2026-11-24; re-read anything that may have moved (and populate `producer_workflow_id` and
any Sonar debt snapshot) before the policy is promoted to `release`.

| Field | Value | Source |
|---|---|---|
| `repository_id` | `1408285582` | `gh api repos/storytold/gridcraft --jq .id` |
| `producer_workflow_id` | `null` (pending `main` registration) | Read with `gh api repos/storytold/gridcraft/actions/workflows/nightly.yml --jq .id` after `nightly.yml` lands on `main` |
| `producer_tooling_revision`, `signer_tooling_revision`, `producer_workflow_ref` | `605d5876…` (craft-common-actions #339) | The callers' pin |
| `manual_actors` / `automatic_signer_actors` | the five release managers / `[]` | release-managers team (id 19837241) members who can push; equals the producer caller's `manual-actors` |
| `max_age_seconds`, `max_artifact_bytes` | `86400`, `2147483648` | Go-live checklist |
| `platforms` | three rows | Suite stable identities (Windows Authenticode publisher subject, Linux OpenPGP release key fingerprint, macOS Apple Developer ID team `DJ6XS33FX8`) |
| `candidate_policy.required_workflows` | CI (`ci.yml`, workflow `380135773`), 1 job (`gate / gate (ubuntu-24.04)`) | Workflow list API and recent green main push run |
| `candidate_policy.sonar` | `storytold` / `storytold_gridcraft`, app `12526` | `sonar-project.properties`; the SonarCloud check on main commits |
| Sonar `issue_types` / `debt` | `VULNERABILITY` / `[]` | Refresh from `api/issues/search` on main before promoting to `release` |

## Ownership and interface

- `.github/workflows/nightly.yml`: default-main registration of the unsigned producer, named
  `GridCraft unsigned nightly`, enabled for manual dispatch by `manual-actors`; no signing secrets
  may be passed or inherited.
- `.github/workflows/nightly-sign.yml`: top-level release registration, enabled for manual dispatch,
  and identical on main and release after reviewed promotion. It rejects wrong repository/event/actual branch,
  including same-name tags. Its constant whole-run concurrency (`gridcraft-nightly-release`) is distinct
  from stable, cancellation is off and `queue: max` must be hosted-qualified. The caller itself passes no
  credentials; only the shared callee's sign jobs bind the `release` environment.
- Shared reusable entrypoints are `nightly-unsigned.yml` and `nightly-release.yml` in
  `storytold/craft-common-actions`. Both must resolve to a **real reviewed public immutable
  commit**; the defining workflow SHA and tooling input must match.

## Local regression checks

`python3 packaging/nightly-release-tests.py` verifies the GridCraft nightly source contract:
- Both callers share one pinned 40-hex commit in `uses` and `tooling-revision`.
- No cache actions, cache setup options, `environment:`, or `secrets` appear in the callers.
- No nightly schedule is registered.
- Producer and signer keep their exact context guards (every `if:` form and continuation line is read).
- Manual actors are exactly the reviewed release managers in both `nightly.yml` and `.github/nightly-signing-policy.json`, with `automatic_signer_actors` empty.
- Root serialization in `nightly-sign.yml` remains literal (`group: gridcraft-nightly-release`, `cancel-in-progress: false`, `queue: max`).
- Installed policy v3 fields, bounds, platforms, candidate workflows, and Sonar identity match `sonar-project.properties` and the callers.
