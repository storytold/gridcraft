# Contributing to GridCraft

## Welcome

Thanks for helping improve GridCraft. Bug fixes, Excel compatibility work, features, tests,
documentation and localization are all welcome. Start with [AGENTS.md](AGENTS.md) for the
project's rules and [docs/architecture.md](docs/architecture.md) for the workspace layout.
The [roadmap](ROADMAP.md) and [ranked gaps](docs/gaps.md) describe current priorities.

Some references in the agent instructions point to `storytold/craftrules` or gitignored
`plan/` notes that external contributors cannot access. Use the public rules and documents
in this repository; ask a maintainer for any additional guidance needed for your change.
You do not need access to those resources to report a problem.

## Before opening an issue

Search existing [issues](https://github.com/storytold/gridcraft/issues) and
[pull requests](https://github.com/storytold/gridcraft/pulls) first. Add useful evidence to an
existing report when it describes the same problem. For questions and general discussion,
visit the [ArtCraft Discord](https://discord.gg/artcraft).

The issue forms suggest these title styles. Replace `area` with the affected component;
these are recommendations, not enforced naming rules.

| Report | Suggested title |
|---|---|
| Bug | `fix(area): concise description` |
| Excel compatibility gap | `Parity: concise description` |
| Feature request | `feat(area): concise description` |

## Reporting bugs and parity gaps

Include the GridCraft version or commit, operating system and platform (including the browser
for web builds), reproduction steps, expected and actual behavior, and relevant error output.
Screenshots and a suspected code location can help. Make the reproduction as small as possible:
cell values, formulas and a short sequence of actions are often enough.

For a parity gap, identify the Excel feature and expected behavior. Include the Excel version
used for comparison and public documentation or observations where available. Behavior can be
observed with synthetic data; do not copy Office code, templates, icons, fonts or other assets.
Keep screenshots of Microsoft software out of the repository.

Use invented data in reproducible workbooks. Do not upload real customer files, personal data,
credentials or confidential material; inspect logs and screenshots before attaching them.
Files committed as tests must follow [AGENTS.md](AGENTS.md): build synthetic workbooks in code,
do not commit Excel-produced files or large binary fixtures.

## Working on an issue

Read the full report and its comments, check for an existing PR, and read the relevant public
project docs before choosing an approach. Work on a feature branch from `main` in your fork.
When substantive work begins, use a short issue comment or a Draft PR to describe the intended
change and verification. Coordinate overlapping work; a comment does not assign an issue.
Before submitting, verify the reported behavior and the result of the fix.

## Development and validation

Use a current stable Rust toolchain with rustfmt and clippy. The CI toolchain also includes
`wasm32-unknown-unknown`. From the repository root:

```sh
cargo run --release -p gridcraft -- --sample sales
cargo fmt --all -- --check
cargo xtask ci
```

GridCraft is a Rust-only Cargo workspace: calculation and workbook crates feed the engine,
with separate UI, MCP and app crates. Follow the layering in [docs/architecture.md](docs/architecture.md).
User-visible actions go through engine commands; keep the UI thin and programmatic calls free
of dialogs. Preserve WASM compatibility; do not add handwritten JavaScript or TypeScript.

The gate required by [AGENTS.md](AGENTS.md) before every commit is `cargo xtask ci`: formatting,
clippy with warnings denied, workspace tests, asset attribution, layering, WASM, and the parity
and keytip checks implemented in `xtask`. The [CI workflow](.github/workflows/ci.yml) runs it on PRs.
Add tests for behavior changes; every crash fix needs a regression test. Production code must
avoid panics and unsafe code, return errors through `Result`, validate untrusted input, and
bound allocations and recursion as described in the Never Crash rules.

For documentation and GitHub configuration, also check Markdown, links, YAML and the
[Issue Forms syntax](https://docs.github.com/en/communities/using-templates-to-encourage-useful-issues-and-pull-requests/syntax-for-issue-forms).
These checks do not replace the documented gate. If you cannot run a check, explain why in the
PR and distinguish it from checks that passed.

## Pull requests

Keep each PR focused on one task, with clear commits that identify it. Explain the problem,
the resulting behavior and the checks you actually ran. Link fully resolved issues with
`Fixes #123`; use `Refs #123` for related or partial work. For UI changes, include a GridCraft
screenshot made with synthetic data; the offscreen snapshot example is documented in
[AGENTS.md](AGENTS.md). Respond to review feedback and rerun relevant checks after changes.

Disclose AI assistance in the PR; the contributor remains responsible for correctness, tests
and licensing. [Contributor credits](docs/contributors.md) include models named in
`Co-Authored-By` trailers. Use a verified identity for the tool actually used: merged
[Codex-assisted work](https://github.com/storytold/gridcraft/commit/96b3ab324471e4dd2bb5d31f48cbd5f406796547)
uses `Co-Authored-By: Codex <noreply@openai.com>`. Do not guess another model's name or email,
invent citations, report unrun tests as passing, or overstate what your evidence establishes.

## Licence and attribution

GridCraft is dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE).
Follow its clean-room rules: behavior comes from public specifications and observation,
not commercial software code or restricted resources. Do not copy GPL/LGPL/AGPL implementations.
Assets must meet [AGENTS.md](AGENTS.md)'s original or redistributable-license requirements and
have an [ATTRIBUTION.md](ATTRIBUTION.md) entry. Font files belong in the separate font repository,
not this one; the ArtCraft brand has its own [trademark terms](docs/brand/LICENSE-brand.txt).

Everyone is credited by GitHub username. For optional names, use the manual process in
[docs/contributors.md](docs/contributors.md). Add only your own entry; do not edit anyone else's
names or the generated `contributors/contributors.json`.
