# Decisions

## Formula localization uses the existing interface preference

PR #98 builds on the persisted `UiState.language` introduced by #31 and extended by #104.
Español selects the Spanish function table and list/decimal separators; there is no competing
locale setting. Formula syntax without a supplied translation table falls back to English.
The editor translates to canonical English before engine commands, and translates back only
for display. API, clipboard and XLSX syntax remain stable. Existing names override aliases.

The revision is based directly on main after #31/#104 and Portuguese interface support (#82)
merged. Those locales retain their existing formula fallback; this change adds only Spanish.
