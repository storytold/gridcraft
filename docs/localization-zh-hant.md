# 繁體中文（台灣）介面 / Traditional Chinese (Taiwan) interface

更新日期 / Updated: 2026-11-20

## 繁體中文

在「檢視」的介面語言選單選擇「繁體中文」，或透過控制通道呼叫
`app.language.set`，參數為 `{"language":"zh-hant"}`。設定會儲存並在下次啟動時保留。
首次啟動時，`zh-TW`、`zh-HK`、`zh-MO` 與 `zh-Hant` 系統語系會選擇繁體中文；
明確指定的 `Hans`／`Hant` 文字系統優先於地區。原本的 `zh`、`zh-CN`、`zh-SG`
與 `zh-Hans` 設定仍使用簡體中文。

翻譯檔位於 `crates/ui-egui/src/i18n/zh_hant.rs`，沿用現有的英文查表機制，
採用「儲存格、活頁簿、工作表、樞紐分析表、資料編輯列、走勢圖、貼上」等台灣用語。
譯文由英文原始字串重新撰寫，並經台灣用語審核；沒有複製其他軟體的翻譯資源。
未翻譯的文字仍顯示英文。公式語法、數值與日期格式、文件內容及命令 ID
沿用既有行為；新增語系不代表整個應用程式已完成在地化。

### 函數說明

`crates/ui-egui/src/i18n/zh_hant_functions.rs` 提供函數目錄中全部 479 則函數說明與
283 個引數名稱的繁中譯文（引數提示、自動完成、插入函數對話方塊及「公式」索引標籤選單），
採用台灣 Excel 用語，例如「傳回、參照、儲存格、陣列、引數、常態分配、變異數、亂數」。
譯文以 #3271（acebd12345）的翻譯為基礎，並依台灣用語修訂。`cargo xtask i18n-report`
會列出目錄已不存在的英文鍵；測試確認每個函數都有繁中說明。

### macOS 選單列

macOS 原生選單列（檔案、編輯、檢視、插入、格式、工具、資料、視窗、說明及 GridCraft 選單）
跟隨介面語言；在「檢視」切換語言後，選單標題與項目會立即改為新語言，所有語言皆適用。
選單標籤集中於 `apps/gridcraft/src/native_menu_tree.rs`，測試確認每個標籤都有繁中譯文。
系統提供的項目（服務、隱藏等）由 macOS 依系統語言顯示。

### 共用 craft-i18n 的交付計畫

目前繁中字表放在應用程式內（`crates/ui-egui/src/i18n/zh_hant.rs` 與 `zh_hant_functions.rs`），
因為 GridCraft 尚未接上共用字表機制。依 craft-contrib #72 的方向，繁中譯文之後改由
`storytold/craft-i18n` 交付，搬移方式如下：

- **介面字表**：`locales/zh-hant/gridcraft.tsv`，三欄格式與現有 TSV 相同（context、英文原文、
  譯文）。英文鍵不變，1,341 筆對應關係原樣搬移，不需要重新翻譯或重新審閱用語。
- **共用詞彙**：沿用 `locales/zh-hant/common.tsv` 既有的詞；應用程式自己的列優先於共用列。
- **函數說明**：craft-i18n 目前沒有應用程式交付函數說明，這 762 列（479 則說明與 283 個
  引數名稱）先留在應用程式內，等共用格式定義了位置再一起搬。
- **依賴**：GridCraft 需先接上 craft-i18n 的建置選項（`CRAFT_I18N_DIR`，見 craft-i18n 的
  `docs/integration.md`），那是另一個變更。接上之後，本 PR 的應用程式內字表會在同一個
  遷移提交中刪除，避免兩份字表並存；順序由維護者決定。

在共用機制接上之前，本 PR 讓繁中介面不必等待就能使用；缺字仍顯示英文，不影響建置、
測試或發布。

### Noto CJK 繁中字型

桌面版繁中介面優先使用已安裝的 **Noto Sans CJK TC**，其次使用支援的繁中系統字型。
請從 [Noto CJK 官方專案](https://github.com/notofonts/noto-cjk) 下載
`Sans/OTF/TraditionalChinese/NotoSansCJKtc-Regular.otf`，保留其 SIL Open Font License。
字型檔不放入 GridCraft 原始碼庫，也不在執行時自動下載。

- Windows：將檔案安裝至 `%LOCALAPPDATA%\Microsoft\Windows\Fonts` 或 `%WINDIR%\Fonts`，
  並保留檔名 `NotoSansCJKtc-Regular.otf`。也可放入使用者字型目錄的 `NotoSansCJKtc`
  子目錄；應用程式會優先讀取這份檔案，避免覆寫正在使用的舊字型。
- macOS：放在 `~/Library/Fonts` 或 `/Library/Fonts`。
- Linux：放在 `~/.local/share/fonts`、`~/.fonts` 或既有的系統 Noto 字型目錄。
  `NotoSansCJK-Regular.ttc` 使用台灣字形的第 3 號字面。

安裝後重新啟動應用程式。找不到繁中字型時會沿用既有後備字型，字形可能不是台灣標準；
網頁版維持現有字型打包方式，此修改不新增瀏覽器字型下載或打包。

## English

Choose **繁體中文** in the View tab's interface-language picker, or call `app.language.set`
through the control channel with `{"language":"zh-hant"}`. The preference persists across launches.
On first launch, `zh-TW`, `zh-HK`, `zh-MO` and `zh-Hant` system locales select Traditional Chinese.
An explicit `Hans` or `Hant` script takes precedence over the region. Existing `zh`, `zh-CN`,
`zh-SG` and `zh-Hans` preferences continue to select Simplified Chinese.

The catalog in `crates/ui-egui/src/i18n/zh_hant.rs` uses the existing English-keyed lookup mechanism
and Taiwan spreadsheet terminology. Translations were written from the English source strings
and reviewed for Taiwan usage; no proprietary localization resources were copied. Untranslated
strings fall back to English. Formula syntax, number/date formats, document content
and command IDs retain their existing behavior. Adding this catalog does not imply complete
application localization.

Worksheet function help is in `crates/ui-egui/src/i18n/zh_hant_functions.rs`: all 479 function
descriptions and 283 argument names in the function catalog (argument hints, autocomplete, Insert
Function and the Formulas-tab menus), in Taiwan Excel terms. It builds on the translations in #3271
by acebd12345, revised for Taiwan usage; a test checks every catalog function has a description.

On macOS the native menu bar follows the interface language: choosing another language in View
relabels every menu title and item at once, for every language. The labels live in
`apps/gridcraft/src/native_menu_tree.rs`, and a test checks each has a Traditional Chinese
translation. System-provided items (Services, Hide…) follow the macOS system language.

### Delivery through craft-i18n

The catalog currently lives in the app (`crates/ui-egui/src/i18n/zh_hant.rs` and
`zh_hant_functions.rs`) because GridCraft has not adopted the shared catalog mechanism yet. Under
the direction in craft-contrib #72 the Traditional Chinese data moves to `storytold/craft-i18n`:

- **Interface rows**: `locales/zh-hant/gridcraft.tsv`, in the same three-column format as the
  existing TSV (context, English source, translation). The English keys do not change, so the
  1,341 pairs move across as they are; no wording is re-translated or re-reviewed.
- **Shared terms**: the existing `locales/zh-hant/common.tsv` rows apply, and this app's rows win
  over the shared ones in the same locale.
- **Function help**: no app delivers function descriptions to craft-i18n today, so these 762 rows
  (479 descriptions and 283 argument names) stay in the app until the shared format has a place
  for them.
- **Dependency**: GridCraft first needs the craft-i18n build option (`CRAFT_I18N_DIR`, see
  craft-i18n's `docs/integration.md`), which is a separate change. After that, the in-app catalogs
  added here are removed in the same migration commit, so the two never coexist. The order is the
  maintainers' call.

Until the shared route lands, this PR makes the Traditional Chinese interface usable without
waiting for it. Missing strings still fall back to English and block neither the build, the tests
nor a release.

### Noto CJK fonts

The native Traditional Chinese interface prefers an installed **Noto Sans CJK TC** face, then
supported Traditional Chinese system faces. Download
`Sans/OTF/TraditionalChinese/NotoSansCJKtc-Regular.otf` from the
[official Noto CJK repository](https://github.com/notofonts/noto-cjk), retaining its SIL Open Font
License. Font binaries stay outside the GridCraft repository and are never downloaded at runtime.

- Windows: install the file in `%LOCALAPPDATA%\Microsoft\Windows\Fonts` or `%WINDIR%\Fonts`,
  keeping the filename `NotoSansCJKtc-Regular.otf`. The `NotoSansCJKtc` subdirectory of the
  per-user font directory is checked first, allowing an updated file without replacing a font
  already in use.
- macOS: use `~/Library/Fonts` or `/Library/Fonts`.
- Linux: use `~/.local/share/fonts`, `~/.fonts` or an existing system Noto font directory.
  The merged `NotoSansCJK-Regular.ttc` uses face index 3 for Taiwan glyph shapes.

Restart after installing. Without a supported TC face, the existing fallback chain remains
available and may use regional glyph shapes other than Taiwan's. Web builds retain the existing
font packaging; this change adds no browser font download or embedding.
