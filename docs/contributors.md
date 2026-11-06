# Contributors in the About window

**About ▸ Contributors** credits everyone who contributed to GridCraft, and **About ▸ Models** credits
the AI models named in `Co-Authored-By` trailers.

## Decision

- Two files, both **compiled into the binary** by the UI crate's `build.rs` (nothing is read from
  disk or the network at run time; the web build has them too):
  - [`contributors/contributors.json`](../contributors/contributors.json): commit stats per GitHub
    username. Never hand-edit.
  - [`contributors/people.toml`](../contributors/people.toml): the names people chose for
    themselves. **Each contributor adds only their own entry**, in their own PR.
- Each contributor gets one line: GitHub username (always), display name (opt-in, or their synced
  GitHub profile name), real name (only if they told us), merged PRs, commits, lines added, lines
  deleted, binary assets added, binary assets removed, first and last commit date.
- The list is shown as a **grab bag** (names flowing as text) or a **table**, in the same order. It
  sorts by name, PRs, commits, lines added, lines deleted, line delta, binary assets added/removed,
  first or last commit date. The name toggle cycles **Username → Display name → Real name**; a
  missing name falls back to `@username`. Alphabetical sorting is case-insensitive and ignores the `@`.

## In GridCraft

- Open it from the macOS menu GridCraft ▸ About GridCraft or Help ▸ Contributors…, or the ribbon's
  Automate ▸ About GridCraft (all platforms); agents: `ui.dialog {"name":"about","tab":"Contributors"}`.

## Credit yourself

Add your entry to `contributors/people.toml` in a PR you commit yourself (every field optional):

```toml
[people.your-github-username]
real_name = "Your Name"
display_name = "Nickname"
sync_github_name = true   # use the name on your public GitHub profile as display name
```

Names show for entries committed by their own user (or by a maintainer with a `consent_source`).
Delete your entry to remove your names. Git author names and emails are never recorded.

## Generated snapshot timing / 產生名單的時機

Adding a `people.toml` entry opts into a name; it does not regenerate commit statistics or verify
the entry itself. The About window only uses names for contributors present in `contributors.json`
with `people_entry = true`. A new contributor's entry can therefore remain absent from the current
snapshot while their PR is awaiting merge. Before publishing a release, maintainers need to refresh
the snapshot with their contributor generator so it includes the merged contribution and confirms
the name the contributor chose. This checkout does not contain that generator. Do not hand-edit the
snapshot or fabricate statistics for an unmerged PR.
The repository-wide stale snapshot and release refresh are tracked in
[#1810](https://github.com/storytold/gridcraft/issues/1810) and
[#1811](https://github.com/storytold/gridcraft/issues/1811).

新增 `people.toml` 項目是同意顯示名稱，不會自動重新產生提交統計或驗證該項目。
「關於」視窗只會對 `contributors.json` 中已列入且具有 `people_entry = true` 的貢獻者
套用名稱。因此，新貢獻者的 PR 尚待合併時，目前的統計快照可能還沒有這位貢獻者。
發布版本前，維護者需要用貢獻者名單產生器更新快照，納入已合併的貢獻，
並確認該名稱是貢獻者本人選擇並提交的。
此原始碼庫未包含該產生器；請勿手動編輯統計快照，或為尚未合併的 PR 填入虛構統計。
全專案統計快照過期與發布流程刷新分別追蹤於
[#1810](https://github.com/storytold/gridcraft/issues/1810) 與
[#1811](https://github.com/storytold/gridcraft/issues/1811)。
