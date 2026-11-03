# 繁體中文（台灣）介面 / Traditional Chinese (Taiwan) interface

更新日期 / Updated: 2026-11-03

## 繁體中文

在「檢視」的介面語言選單選擇「繁體中文」，或透過控制通道呼叫
`app.language.set`，參數為 `{"language":"zh-hant"}`。設定會儲存並在下次啟動時保留。
首次啟動時，`zh-TW`、`zh-HK`、`zh-MO` 與 `zh-Hant` 系統語系會選擇繁體中文；
明確指定的 `Hans`／`Hant` 文字系統優先於地區。原本的 `zh`、`zh-CN`、`zh-SG`
與 `zh-Hans` 設定仍使用簡體中文。

翻譯檔位於 `crates/ui-egui/src/i18n/zh_hant.rs`，沿用現有的英文查表機制，
採用「儲存格、活頁簿、工作表、樞紐分析表、資料編輯列、走勢圖、貼上」等台灣用語。
譯文由英文原始字串重新撰寫，並經台灣用語審核；沒有複製其他軟體的翻譯資源。
未翻譯的文字仍顯示英文。函數說明、公式語法、數值與日期格式、文件內容及命令 ID
沿用既有行為；新增語系不代表整個應用程式已完成在地化。

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
strings fall back to English. Function help, formula syntax, number/date formats, document content
and command IDs retain their existing behavior. Adding this catalog does not imply complete
application localization.

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
