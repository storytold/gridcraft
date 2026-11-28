# Windows validation portability fixes

This change keeps two platform-specific CI failures separate from the Analysis
ToolPak and chart feature contributions.

`IMSQRT` formats complex results as text. The Windows CRT sine result at the
half-angle for an imaginary unit differed at the last displayed digit from the
existing exact contract. The implementation now uses pinned `libm` sine/cosine
for that calculation. Regression tests preserve the exact `i`, `-i` and `j`
results without increasing tolerances. The existing half-pi residual behavior
for negative real inputs is retained. This does not assert bitwise equality of
every intermediate operation or arbitrary complex input across all platforms.

The CLI directory traversal tracks Unix device/inode identities through `seen`.
On other platforms it uses the existing depth limit, so the argument is only
forwarded recursively. An explicit non-Unix read of that argument makes this
platform distinction clear to Clippy without changing traversal behavior.

The fixes passed the recorded 2026-11-04 Windows integration runs, including
exact engineering tests and all ten CI gates in the feature producer trees.
The standalone extraction on main `c4029538` also passed all ten local
`cargo xtask ci` gates on Fedora on 2026-11-05, including the exact-output
regression and WASM. The CI log's compiler, parity and keytips paths match
the standalone checkout. The Windows results describe the earlier recorded
producer sources; they are not a second Windows run of the extracted branch.
