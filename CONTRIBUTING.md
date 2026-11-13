# Contributing to GridCraft

Thank you for helping improve GridCraft. Start with [AGENTS.md](AGENTS.md), which contains
the repository's contributor and coding rules, and [the architecture](docs/architecture.md).
The project is a clean-room Rust spreadsheet for native platforms and WebAssembly.

## Issues and pull requests

For a bug, include the GridCraft version, platform, steps to reproduce, expected/actual
behavior and the smallest synthetic example that demonstrates the problem. Remove private
data. Prefer commands/formulas that another contributor can replay.

Keep a pull request focused on a concrete behavior. Explain the problem, implementation,
validation performed and any known limitations. Link related documentation and report checks
that could not run, including their actual failure. Do not describe an unexecuted check as passed.
Update the relevant area documentation when behavior changes; keep Excel parity distinct from
GridCraft-specific extensions such as [the actuarial pack](docs/actuarial.md).

## Code and source material

- Write Rust and preserve the [crate layering](docs/architecture.md#workspace-and-layering).
  User-visible actions use the existing command layer; the UI remains thin.
- No unsafe code or production panics. Validate hostile numeric input, bound allocations and
  iterations, and return existing error types. Include regression tests for substantive bugs.
- Preserve WebAssembly support. Follow existing function, command and UI patterns.
- Maintain clean-room provenance: do not copy implementation code from GPL/AGPL/LGPL software,
  read/disassemble proprietary application bundles, or commit proprietary vendor assets or
  Excel-produced fixtures. Public behavior and published mathematical methods can guide an
  original implementation. Synthetic fixtures should be constructed in code.
- Assets require a row in [ATTRIBUTION.md](ATTRIBUTION.md). Fonts are optional build inputs from
  `storytold/craft-fonts`, not files bundled into this repository. Existing license terms are in
  [LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE) and [NOTICE](NOTICE).

## Numerical and actuarial contributions

Document parameterizations, units, payment timing, probability definitions, identifiable
domains, truncation, convergence criteria and resource bounds. Cite primary mathematical
references and include analytical or independently reproducible numerical cases. Check
probability conservation and invariants; distinguish parameter uncertainty, process risk and
Monte Carlo error. Returning an error is preferable to silently regularizing an undefined fit.
Do not imply regulatory compliance or scientific validation from the existence of a formula.

## Before committing

Run the repository gate:

```sh
cargo xtask ci
```

It checks formatting, Clippy, tests/doctests, assets, dependency layers, WebAssembly and generated
metadata. The full command and optional font setup are described in [AGENTS.md](AGENTS.md).
For investigation, narrow checks such as `cargo test -p gridcraft-functions actuarial` are useful;
they do not replace the full gate. Use a short task-oriented commit message as described there.

Contributor credits are opt-in beyond the GitHub username. Follow [the contributor guide](docs/contributors.md)
and edit only your own entry when you choose to provide additional credit information.
