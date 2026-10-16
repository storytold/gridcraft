//! Interface translations. Command ids and document text stay stable; only interface chrome is
//! translated. Untranslated labels fall back to English, so coverage can grow incrementally.
//!
//! The interface language is a preference ([`crate::UiState::language`]): on first run it defaults
//! to the system's language ([`Language::system`]), and the View tab switches it by hand.
//!
//! CJK fonts stay system-provided: [`crate::theme`] appends available CJK faces to every family so
//! Japanese, Chinese and Korean labels and workbook text can render without bundling font files.

use serde::{Deserialize, Serialize};

mod cs;
mod de;
mod es;
mod fr;
mod ko;
mod pt;
mod ru;
mod sr;
mod zh;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    En,
    Es,
    Zh,
    Ja,
    Ko,
    Ru,
    #[serde(rename = "pt")]
    PtBr,
    De,
    Cs,
    Fr,
    Sr,
}

impl Language {
    pub const ALL: [Self; 11] = [Self::En, Self::Es, Self::Zh, Self::Ja, Self::Ko, Self::Ru, Self::PtBr, Self::De, Self::Cs, Self::Fr, Self::Sr];

    /// The language's own name, shown in the switcher.
    pub fn name(self) -> &'static str {
        match self {
            Self::En => "English",
            Self::Es => "Español",
            Self::Zh => "简体中文",
            Self::Ja => "日本語",
            Self::Ko => "한국어",
            Self::Ru => "Русский",
            Self::PtBr => "Português (Brasil)",
            Self::De => "Deutsch",
            Self::Cs => "Čeština",
            Self::Fr => "Français",
            Self::Sr => "Srpski",
        }
    }

    /// The language's code, as saved in `ui.json` and taken by `app.language.set`.
    pub const fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Es => "es",
            Self::Zh => "zh",
            Self::Ja => "ja",
            Self::Ko => "ko",
            Self::Ru => "ru",
            Self::PtBr => "pt",
            Self::De => "de",
            Self::Cs => "cs",
            Self::Fr => "fr",
            Self::Sr => "sr",
        }
    }

    pub fn parse(code: &str) -> Option<Self> {
        Self::from_tag(code)
    }

    fn from_tag(tag: &str) -> Option<Self> {
        match tag.trim().split(['-', '_']).next()?.to_ascii_lowercase().as_str() {
            "en" => Some(Self::En),
            "es" => Some(Self::Es),
            "zh" => Some(Self::Zh),
            "ja" => Some(Self::Ja),
            "ko" => Some(Self::Ko),
            "ru" => Some(Self::Ru),
            "pt" => Some(Self::PtBr),
            "de" => Some(Self::De),
            "cs" => Some(Self::Cs),
            "fr" => Some(Self::Fr),
            "sr" => Some(Self::Sr),
            _ => None,
        }
    }

    /// The system's preferred interface language, best effort; English when it is unknown or
    /// unsupported. Used as the first-run default while nothing is saved.
    pub fn system() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            sys_locale::get_locale().and_then(|tag| Self::from_tag(&tag)).unwrap_or(Self::En)
        }
        #[cfg(target_arch = "wasm32")]
        {
            web_sys::window().and_then(|window| window.navigator().language()).and_then(|tag| Self::from_tag(&tag)).unwrap_or(Self::En)
        }
    }

    pub fn formula_locale(self) -> gridcraft_engine::formula::FormulaLocale {
        if self == Self::Es { gridcraft_engine::formula::FormulaLocale::Es } else { gridcraft_engine::formula::FormulaLocale::En }
    }

    /// Translates an interface label; untranslated labels fall back to English.
    ///
    /// A label that needs two translations (Home's text "Orientation" vs Page Layout's print
    /// "Orientation") is keyed `context|label`: English shows only the part after the `|`.
    pub fn tr(self, text: &str) -> &str {
        self.lookup(text).unwrap_or_else(|| text.rsplit_once('|').map_or(text, |(_, label)| label))
    }

    /// A ribbon tab's name. Where a language names the tab differently from a command with the
    /// same English label (Russian: the Insert tab is «Вставка», the Insert button «Вставить»), its
    /// catalog keys the tab `Tab|name`; every other language falls back to the plain label.
    pub fn tr_tab(self, tab: &str) -> &str {
        self.lookup(&format!("Tab|{tab}")).unwrap_or_else(|| self.tr(tab))
    }

    /// The catalog entry for `text`, if this language has one.
    fn lookup(self, text: &str) -> Option<&'static str> {
        match self {
            Self::En => None,
            Self::Es => es::translate(text),
            Self::Ja => JAPANESE.iter().find(|(english, _)| *english == text).map(|(_, translated)| *translated),
            Self::Zh => zh::translate(text),
            Self::Ko => ko::translate(text),
            Self::Ru => ru::translate(text),
            Self::PtBr => pt::translate(text),
            Self::De => de::translate(text),
            Self::Cs => cs::translate(text),
            Self::Fr => fr::translate(text),
            Self::Sr => sr::translate(text),
        }
    }
}

/// Deserializes a saved language leniently: an unknown or malformed value (a language removed in a
/// later version, a hand-edited `ui.json`) reads as English instead of failing the whole `UiState`,
/// which would reset every other preference with it.
pub fn lenient<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Language, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Code(String),
        Other(serde::de::IgnoredAny),
    }
    Ok(match Raw::deserialize(d)? {
        Raw::Code(code) => Language::parse(&code).unwrap_or_default(),
        Raw::Other(_) => Language::default(),
    })
}

/// The language a saved `ui.json` asks for, if it names a supported one. The desktop app uses it
/// to tell "saved English" from "nothing usable saved" (first run, an older file, an unknown
/// value), which starts from [`Language::system`] instead.
pub fn saved_language(ui_json: &serde_json::Value) -> Option<Language> {
    ui_json.get("language").and_then(serde_json::Value::as_str).and_then(Language::parse)
}

/// egui context-data key holding the language of the frame being drawn. Widgets that only get a
/// `Ui` (not the app) read it to translate their labels; [`set_current`] writes it once per frame.
fn lang_key() -> egui::Id {
    egui::Id::new("gridcraft.language")
}

/// The language the current frame is drawing in. `En` when unset.
pub fn current(ctx: &egui::Context) -> Language {
    ctx.data(|d| d.get_temp(lang_key())).unwrap_or_default()
}

/// Sets the language for the current frame; call once per frame before anything is drawn.
pub fn set_current(ctx: &egui::Context, language: Language) {
    ctx.data_mut(|d| d.insert_temp(lang_key(), language));
}

/// English label → Japanese. Keys are the exact interface strings (command ids, file names and
/// document text are never translated); anything absent falls back to English.
const JAPANESE: &[(&str, &str)] = &[
    ("Paste", "貼り付け"),
    ("Paste Values", "値の貼り付け"),
    ("Paste Formulas", "数式の貼り付け"),
    ("Paste Formatting", "書式の貼り付け"),
    ("Transpose", "行列を入れ替え"),
    ("Paste Link", "リンクの貼り付け"),
    ("Paste Special…", "形式を選択して貼り付け…"),
    ("Insert Copied Cells", "コピーしたセルの挿入"),
    ("Insert Copied Cells…", "コピーしたセルの挿入…"),
    ("Insert Cut Cells", "切り取ったセルの挿入"),
    ("Insert Paste", "挿入貼り付け"),
    ("Cut (⌘X)", "切り取り (⌘X)"),
    ("Copy (⌘C)", "コピー (⌘C)"),
    ("Format Painter (double-click to keep it on)", "書式のコピー/貼り付け (ダブルクリックで固定)"),
    ("Bold (⌘B)", "太字 (⌘B)"),
    ("Italic (⌘I)", "斜体 (⌘I)"),
    ("Underline (⌘U)", "下線 (⌘U)"),
    ("Double Underline", "二重下線"),
    ("Strikethrough", "取り消し線"),
    ("Underline", "下線"),
    ("Increase Font Size", "フォントの拡大"),
    ("Decrease Font Size", "フォントの縮小"),
    ("Wrap Text", "折り返して全体を表示"),
    ("Merge & Center", "セルを結合して中央揃え"),
    ("Merge Cells", "セルの結合"),
    ("Merge Across", "横方向に結合"),
    ("Unmerge Cells", "セル結合の解除"),
    ("Fill Color", "塗りつぶしの色"),
    ("Font Color", "フォントの色"),
    ("Borders", "罫線"),
    ("Top\nAlign", "上揃え"),
    ("Middle\nAlign", "中央揃え (縦)"),
    ("Bottom\nAlign", "下揃え"),
    ("Left", "左"),
    ("Right", "右"),
    ("Center", "中央"),
    ("Horizontal", "水平"),
    ("Vertical Text", "縦書き"),
    ("Decrease Indent", "インデントを減らす"),
    ("Increase Indent", "インデントを増やす"),
    ("Orientation", "方向"),
    ("Page Layout|Orientation", "印刷の向き"),
    ("Angle Counterclockwise", "左回りに回転"),
    ("Angle Clockwise", "右回りに回転"),
    ("Rotate Text Up", "文字列を上に回転"),
    ("Rotate Text Down", "文字列を下に回転"),
    ("Accounting Number Format", "通貨表示形式"),
    ("Percent Style", "パーセント表示"),
    ("Comma Style", "桁区切りスタイル"),
    ("Increase Decimal", "小数点以下の桁数を増やす"),
    ("Decrease Decimal", "小数点以下の桁数を減らす"),
    ("More Accounting Formats…", "その他の通貨表示形式…"),
    ("Conditional\nFormatting", "条件付き書式"),
    ("Conditional Formatting", "条件付き書式"),
    ("Format\nas Table", "テーブルとして書式設定"),
    ("Cell\nStyles", "セルのスタイル"),
    ("Format Cells…", "セルの書式設定…"),
    ("Format Cell Alignment…", "セルの配置…"),
    ("Format\nPane", "書式ウィンドウ"),
    ("Format", "書式"),
    ("Insert", "挿入"),
    ("Insert Cells…", "セルの挿入…"),
    ("Delete", "削除"),
    ("Delete Cells…", "セルの削除…"),
    ("Insert Sheet Rows", "シートの行を挿入"),
    ("Insert Sheet Columns", "シートの列を挿入"),
    ("Delete Sheet Rows", "シートの行を削除"),
    ("Delete Sheet Columns", "シートの列を削除"),
    ("Row Height…", "行の高さ…"),
    ("AutoFit Row Height", "行の高さを自動調整"),
    ("Column Width…", "列の幅…"),
    ("AutoFit Column Width", "列の幅を自動調整"),
    ("Default Width…", "標準の幅…"),
    ("Hide Rows", "行を表示しない"),
    ("Unhide Rows", "再表示"),
    ("Hide Columns", "列の表示/非表示"),
    ("Unhide Columns", "再表示"),
    ("Lock Cell", "セルのロック"),
    ("Clear", "クリア"),
    ("Clear All", "すべてクリア"),
    ("Clear Formats", "書式のクリア"),
    ("Clear Contents", "数式と値のクリア"),
    ("Clear Comments and Notes", "コメントとメモのクリア"),
    ("Clear Hyperlinks", "ハイパーリンクのクリア"),
    ("AutoSum", "オート SUM"),
    ("AutoSum (⌘⇧T)", "オート SUM (⌘⇧T)"),
    ("Flash\nFill", "フラッシュ フィル"),
    ("Flash Fill", "フラッシュ フィル"),
    ("Fill", "フィル"),
    ("Clear Print Area", "印刷範囲のクリア"),
    ("Find &\nSelect", "検索と選択"),
    ("Find…", "検索…"),
    ("Replace…", "置換…"),
    ("Go To…", "ジャンプ…"),
    ("Go To Special…", "セルを選択…"),
    ("PivotTable", "ピボットテーブル"),
    ("Recommended\nCharts", "推奨グラフ"),
    ("Table", "テーブル"),
    ("Pictures", "画像"),
    ("Shapes", "図形"),
    ("Icons", "アイコン"),
    ("Text\nBox", "テキスト ボックス"),
    ("Comment", "コメント"),
    ("New\nComment", "新しいコメント"),
    ("New Note", "新しいメモ"),
    ("Show/Hide Note", "メモの表示/非表示"),
    ("Header &\nFooter", "ヘッダーとフッター"),
    ("Text to\nColumns", "区切り位置"),
    ("Link", "リンク"),
    ("Symbol", "記号"),
    ("Sparklines", "スパークライン"),
    ("Checkbox", "チェックボックス"),
    ("Insert checkboxes in the selected cells", "選択したセルにチェックボックスを挿入"),
    ("Header & Footer", "ヘッダーとフッター"),
    ("Draw with ink", "インクで描画"),
    ("Eraser", "消しゴム"),
    ("Ink to\nShape", "インクを図形に変換"),
    ("Select\nObjects", "オブジェクトの選択"),
    ("Themes", "テーマ"),
    ("Margins", "余白"),
    ("Left Margin", "左余白"),
    ("Right Margin", "右余白"),
    ("Top Margin", "上余白"),
    ("Bottom Margin", "下余白"),
    ("Header Margin", "ヘッダーの余白"),
    ("Footer Margin", "フッターの余白"),
    ("Size", "サイズ"),
    ("Print\nArea", "印刷範囲"),
    ("Set Print Area", "印刷範囲の設定"),
    ("Breaks", "改ページ"),
    ("Insert Page Break", "改ページの挿入"),
    ("Remove Page Break", "改ページの解除"),
    ("Reset All Page Breaks", "すべての改ページを解除"),
    ("Print\nTitles", "印刷タイトル"),
    ("Sheet\nRight-to-Left", "シートを右から左へ"),
    ("Sheet Right-to-Left", "シートを右から左へ"),
    ("Narrow", "狭い"),
    ("Wide", "広い"),
    ("Normal", "標準"),
    ("Portrait", "縦"),
    ("Landscape", "横"),
    ("Custom Margins…", "ユーザー設定の余白…"),
    ("Insert\nFunction", "関数の挿入"),
    ("Name\nManager", "名前の管理"),
    ("Define Name", "名前の定義"),
    ("Use in Formula", "数式で使用"),
    ("Create from Selection", "選択範囲から作成"),
    ("Trace Precedents", "参照元のトレース"),
    ("Trace Dependents", "参照先のトレース"),
    ("Remove Arrows", "矢印の解除"),
    ("Show Formulas", "数式の表示"),
    ("Error Checking", "エラーチェック"),
    ("Evaluate Formula", "数式の検証"),
    ("Watch Window", "ウォッチ ウィンドウ"),
    ("Calculation\nOptions", "計算方法の設定"),
    ("Calculate Now", "再計算実行"),
    ("Calculate Sheet", "シートの再計算"),
    ("Automatic", "自動"),
    ("Automatic Except for Data Tables", "データ テーブル以外は自動"),
    ("Manual", "手動"),
    ("Sort &\nFilter", "並べ替えとフィルター"),
    ("Sort", "並べ替え"),
    ("Sort A to Z", "昇順"),
    ("Sort Z to A", "降順"),
    ("Custom Sort…", "ユーザー設定の並べ替え…"),
    ("Filter", "フィルター"),
    ("Reapply", "再適用"),
    ("Get Data\n(Text/CSV)", "データの取得 (テキスト/CSV)"),
    ("Remove\nDuplicates", "重複の削除"),
    ("Data\nValidation", "データの入力規則"),
    ("Data Validation", "データの入力規則"),
    ("Data Validation…", "データの入力規則…"),
    ("Circle Invalid Data", "無効なデータをマーク"),
    ("Clear Validation", "入力規則のクリア"),
    ("Group", "グループ化"),
    ("Ungroup", "グループ解除"),
    ("Show Detail", "詳細データの表示"),
    ("Hide Detail", "詳細を表示しない"),
    ("Subtotal", "小計"),
    ("Data\nAnalysis", "データ分析"),
    ("Data Analysis", "データ分析"),
    ("Solver", "ソルバー"),
    ("What-If\nAnalysis", "What-If 分析"),
    ("Data Table…", "データ テーブル…"),
    ("Scenario Manager…", "シナリオ マネージャー…"),
    ("Goal Seek…", "ゴール シーク…"),
    ("Forecast\nSheet", "予測シート"),
    ("Spelling", "スペルチェック"),
    ("Check\nAccessibility", "アクセシビリティ チェック"),
    ("Threaded Comments", "スレッド化されたコメント"),
    ("Show\nComments", "コメントの表示"),
    ("Previous", "前へ"),
    ("Next", "次へ"),
    ("Previous Comment", "前のコメント"),
    ("Next Comment", "次のコメント"),
    ("Show All Notes", "すべてのメモを表示"),
    ("Convert to Comments", "コメントに変換"),
    ("Protect\nWorkbook", "ブックの保護"),
    ("Allow Edit\nRanges", "範囲の編集を許可"),
    ("Allow Edit Ranges", "範囲の編集を許可"),
    ("Unprotect\nSheet", "シート保護の解除"),
    ("Protect Sheet…", "シートの保護…"),
    ("Comments", "コメント"),
    ("Notes", "メモ"),
    ("Page Break\nPreview", "改ページ プレビュー"),
    ("Page\nLayout", "ページ レイアウト"),
    ("Formula Bar", "数式バー"),
    ("Gridlines", "目盛線"),
    ("Headings", "見出し"),
    ("Zoom", "ズーム"),
    ("100%", "100%"),
    ("Zoom to\nSelection", "選択範囲を拡大"),
    ("Freeze\nPanes", "ウィンドウ枠の固定"),
    ("Freeze Panes", "ウィンドウ枠の固定"),
    ("Freeze Top Row", "最上行の固定"),
    ("Freeze First Column", "最左列の固定"),
    ("Unfreeze Panes", "ウィンドウ枠固定の解除"),
    ("Dark Mode", "ダーク モード"),
    ("Display theme", "表示テーマ"),
    ("Animations", "アニメーション"),
    (
        "Glide the selection outline to the cell you move to instead of jumping there",
        "選択範囲の枠を、移動先のセルまで滑らかに動かします（瞬時には切り替えません）",
    ),
    ("System", "システム"),
    ("Light", "ライト"),
    ("Dark", "ダーク"),
    ("Interface language", "表示言語"),
    ("Interface size", "表示サイズ"),
    ("Auto", "自動"),
    ("Drag to freeze columns", "ドラッグして列を固定"),
    ("Drag to freeze rows", "ドラッグして行を固定"),
    ("Command\nPalette", "コマンド パレット"),
    ("Agent\nControl", "エージェント制御"),
    ("Action\nJournal", "操作ジャーナル"),
    ("About\nGridCraft", "GridCraft について"),
    ("Share", "共有"),
    ("Add Chart\nElement", "グラフ要素を追加"),
    ("Change\nChart Type", "グラフの種類の変更"),
    ("Switch\nRow/Column", "行/列の切り替え"),
    ("Select\nData", "データの選択"),
    ("Data Labels: None", "データ ラベル: なし"),
    ("Data Labels: Show", "データ ラベル: 表示"),
    ("Legend: None", "凡例: なし"),
    ("Legend: Bottom", "凡例: 下"),
    ("Legend: Top", "凡例: 上"),
    ("Legend: Right", "凡例: 右"),
    ("Gridlines: None", "目盛線: なし"),
    ("Gridlines: Show", "目盛線: 表示"),
    ("Chart\nTitle", "グラフ タイトル"),
    ("Delete\nChart", "グラフの削除"),
    ("Convert\nto Range", "範囲に変換"),
    ("Table\nStyles", "テーブル スタイル"),
    ("Top 10 Items", "上位 10 項目"),
    ("Top 10%", "上位 10%"),
    ("Bottom 10 Items", "下位 10 項目"),
    ("Bottom 10%", "下位 10%"),
    ("Above Average", "平均より上"),
    ("Below Average", "平均より下"),
    ("Clear Rules from Entire Sheet", "シート全体からルールをクリア"),
    ("Clear Rules from Selected Cells", "選択したセルからルールをクリア"),
    ("Constants", "定数"),
    ("Data Bars", "データ バー"),
    ("Color Scales", "カラー スケール"),
    ("Icon Sets", "アイコン セット"),
    ("Home", "ホーム"),
    ("Draw", "描画"),
    ("Page Layout", "ページ レイアウト"),
    ("Formulas", "数式"),
    ("Data", "データ"),
    ("Review", "レビュー"),
    ("View", "表示"),
    ("Automate", "自動化"),
    ("Table Design", "テーブル デザイン"),
    ("Chart Design", "グラフ デザイン"),
    ("Ready", "準備完了"),
    ("Enter", "入力"),
    ("Edit", "編集"),
    ("Point", "ポイント"),
    ("Calculate", "計算"),
    ("Select destination and press Enter or choose Paste", "貼り付け先を選び、Enter キーを押すか [貼り付け] を選びます"),
    ("General", "標準"),
    ("Number", "数値"),
    ("Currency", "通貨"),
    ("Accounting", "会計"),
    ("Date", "日付"),
    ("Time", "時刻"),
    ("Percentage", "パーセンテージ"),
    ("Fraction", "分数"),
    ("Scientific", "指数"),
    ("Text", "文字列"),
    ("Special", "特殊"),
    ("Custom", "ユーザー設定"),
    ("Short Date", "日付"),
    ("Long Date", "長い日付"),
    ("More Number Formats…", "その他の表示形式…"),
    ("Financial", "財務"),
    ("Logical", "論理"),
    ("Date &\nTime", "日付/時刻"),
    ("Lookup &\nReference", "検索/行列"),
    ("Math &\nTrig", "数学/三角"),
    ("More\nFunctions", "その他の関数"),
    ("Sheet Options|View", "画面"),
    ("Print", "印刷"),
    // Page Layout view and the Header & Footer tab.
    ("Click to add data", "クリックしてデータを追加"),
    ("Add header", "ヘッダーの追加"),
    ("Add footer", "フッターの追加"),
    ("(none)", "(なし)"),
    ("Header", "ヘッダー"),
    ("Footer", "フッター"),
    ("Page\nNumber", "ページ\n番号"),
    ("Number\nof Pages", "ページ数"),
    ("Current\nDate", "現在の\n日付"),
    ("Current\nTime", "現在の\n時刻"),
    ("File\nPath", "ファイルの\nパス"),
    ("File\nName", "ファイル名"),
    ("Sheet\nName", "シート名"),
    ("Go to\nHeader", "ヘッダーに\n移動"),
    ("Go to\nFooter", "フッターに\n移動"),
    ("Page Number", "ページ番号"),
    ("Number of Pages", "ページ数"),
    ("Current Date", "現在の日付"),
    ("Current Time", "現在の時刻"),
    ("File Path", "ファイルのパス"),
    ("File Name", "ファイル名"),
    ("Sheet Name", "シート名"),
    ("Go to Header", "ヘッダーに移動"),
    ("Different First Page", "先頭ページのみ別指定"),
    ("Different Odd & Even Pages", "奇数/偶数ページ別指定"),
    ("Go to Footer", "フッターに移動"),
    ("Ruler", "ルーラー"),
    // File › Backstage view.
    ("File", "ファイル"),
    ("Back", "戻る"),
    ("New", "新規"),
    ("Open", "開く"),
    ("Info", "情報"),
    ("Save", "上書き保存"),
    ("Save As", "名前を付けて保存"),
    ("Export", "エクスポート"),
    ("Close", "閉じる"),
    ("Options", "オプション"),
    ("Blank workbook", "空白のブック"),
    ("Recent", "最近使ったアイテム"),
    ("No recent workbooks yet.", "最近使ったブックはまだありません。"),
    ("More workbooks", "その他のブック"),
    ("Modified", "更新日時"),
    ("Not available", "利用できません"),
    ("Just now", "たった今"),
    ("Yesterday", "昨日"),
    ("{n} min ago", "{n} 分前"),
    ("{n} h ago", "{n} 時間前"),
    ("{n} days ago", "{n} 日前"),
    ("{n} weeks ago", "{n} 週間前"),
    ("{n} months ago", "{n} か月前"),
    ("{n} years ago", "{n} 年前"),
    ("Browse…", "参照…"),
    ("Not saved yet", "まだ保存されていません"),
    ("Location", "場所"),
    ("Sheets", "シート"),
    ("Protect Workbook", "ブックの保護"),
    (
        "Sheet structure is locked: sheets can't be added, moved, renamed or deleted.",
        "シートの構成はロックされています。シートの追加、移動、名前の変更、削除はできません。",
    ),
    (
        "Lock the sheet structure so sheets can't be added, moved, renamed or deleted.",
        "シートの構成をロックして、シートの追加、移動、名前の変更、削除をできないようにします。",
    ),
    ("Properties", "プロパティ"),
    ("Title", "タイトル"),
    ("Subject", "件名"),
    ("Author", "作成者"),
    ("Company", "会社名"),
    ("Keywords", "キーワード"),
    ("Description", "説明"),
    ("No workbook is open.", "ブックが開かれていません。"),
    ("Save as type", "ファイルの種類"),
    ("Excel Workbook (*.xlsx)", "Excel ブック (*.xlsx)"),
    ("CSV UTF-8 (Comma delimited) (*.csv)", "CSV UTF-8 (コンマ区切り) (*.csv)"),
    ("Web Page (*.html)", "Web ページ (*.html)"),
    ("PDF (*.pdf)", "PDF ファイル (*.pdf)"),
    ("Download", "ダウンロード"),
    (
        "A CSV file, web page or PDF is a copy: the workbook keeps its own file.",
        "CSV ファイル、Web ページ、PDF はコピーです。ブックは元のファイルのままです。",
    ),
    ("Save the workbook under another name or in another folder.", "ブックを別の名前または別のフォルダーに保存します。"),
    ("Print Active Sheets", "作業中のシートを印刷"),
    ("Print Entire Workbook", "ブック全体を印刷"),
    ("Fit Sheet on One Page", "シートを 1 ページに印刷"),
    ("Page Setup…", "ページ設定…"),
    ("Page {page} of {pages}", "{page} / {pages} ページ"),
    ("Nothing to print.", "印刷する内容がありません。"),
    ("Previous page", "前のページ"),
    ("Next page", "次のページ"),
    ("Create PDF", "PDF の作成"),
    ("Save the sheet as a PDF document that keeps its page layout.", "ページ レイアウトを保ったまま、シートを PDF ドキュメントとして保存します。"),
    ("Change File Type", "ファイルの種類の変更"),
    ("CSV (Comma delimited)", "CSV (コンマ区切り)"),
    ("Save the active sheet as comma-separated text.", "作業中のシートをコンマ区切りのテキストとして保存します。"),
    ("Web Page (.html)", "Web ページ (.html)"),
    ("Save the workbook as a web page.", "ブックを Web ページとして保存します。"),
    ("Options|General", "全般"),
    ("Options|Save", "保存"),
    ("Show formula bar", "数式バーを表示する"),
    ("Enable AutoComplete for cell values", "セルの値にオートコンプリートを使用する"),
    ("Save .xlsx workbooks automatically after every change", "変更のたびに .xlsx ブックを自動的に保存する"),
    ("Workbook calculation", "ブックの計算"),
    ("Enable iterative calculation", "反復計算を行う"),
    ("Calculation settings are saved with the workbook.", "計算の設定はブックと共に保存されます。"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translations_are_unique_and_preserve_unknown_text() {
        for (i, (en, ja)) in JAPANESE.iter().enumerate() {
            assert!(!ja.is_empty());
            assert!(JAPANESE.iter().take(i).all(|(other, _)| en != other));
            assert_eq!(Language::En.tr(en), en.rsplit_once('|').map_or(*en, |(_, label)| label));
        }
        for (english, _) in JAPANESE {
            assert!(zh::translate(english).is_some_and(|text| !text.is_empty()), "missing Simplified Chinese: {english}");
            assert!(ko::translate(english).is_some_and(|text| !text.is_empty()), "missing Korean: {english}");
            assert!(ru::translate(english).is_some_and(|text| !text.is_empty()), "missing Russian: {english}");
            assert!(pt::translate(english).is_some_and(|text| !text.is_empty()), "missing Portuguese: {english}");
            assert!(es::translate(english).is_some_and(|text| !text.is_empty()), "missing Spanish: {english}");
            assert!(de::translate(english).is_some_and(|text| !text.is_empty()), "missing German: {english}");
            assert!(cs::translate(english).is_some_and(|text| !text.is_empty()), "missing Czech: {english}");
            assert!(fr::translate(english).is_some_and(|text| !text.is_empty()), "missing French: {english}");
            assert!(sr::translate(english).is_some_and(|text| !text.is_empty()), "missing Serbian: {english}");
        }
        assert_eq!(Language::Ja.tr("Data"), "データ");
        assert_eq!(Language::Zh.tr("Data"), "数据");
        assert_eq!(Language::Ko.tr("Data"), "데이터");
        assert_eq!(Language::Ru.tr("Data"), "Данные");
        // The Insert tab and the Insert command have different Russian names; elsewhere the tab
        // name is the plain label.
        assert_eq!(Language::Ru.tr_tab("Insert"), "Вставка");
        assert_eq!(Language::Ru.tr("Insert"), "Вставить");
        assert_eq!(Language::Ru.tr_tab("Data"), "Данные");
        assert_eq!(Language::Es.tr_tab("Insert"), Language::Es.tr("Insert"));
        assert_eq!(Language::En.tr_tab("Insert"), "Insert");
        assert_eq!(Language::PtBr.tr("Data"), "Dados");
        assert_eq!(Language::Es.tr("Data"), "Datos");
        assert_eq!(Language::Es.tr("Freeze Panes"), "Inmovilizar paneles");
        assert_eq!(Language::Es.tr("Sheet1!A1"), "Sheet1!A1");
        assert_eq!(Language::De.tr("Data"), "Daten");
        assert_eq!(Language::De.tr("Sheet1!A1"), "Sheet1!A1");
        assert_eq!(Language::Fr.tr("Data"), "Données");
        assert_eq!(Language::Fr.tr("Sheet1!A1"), "Sheet1!A1");
        assert_eq!(Language::PtBr.tr("Date"), "Data");
        assert_eq!(Language::PtBr.tr("Sheet1!A1"), "Sheet1!A1");
        assert_eq!(Language::Ja.tr("Sheet1!A1"), "Sheet1!A1");
        assert_eq!(Language::Zh.tr("Sheet1!A1"), "Sheet1!A1");
        assert_eq!(Language::Ko.tr("Sheet1!A1"), "Sheet1!A1");
        assert_eq!(Language::Ru.tr("Sheet1!A1"), "Sheet1!A1");
        // Czech "Data" is spelled as in English, so probe it with a label that differs.
        assert_eq!(Language::Cs.tr("Data"), "Data");
        assert_eq!(Language::Cs.tr("Formulas"), "Vzorce");
        assert_eq!(Language::Cs.tr("Sheet1!A1"), "Sheet1!A1");
        // A `context|label` key shows only its label in English and splits the translation.
        assert_eq!(Language::En.tr("Page Layout|Orientation"), "Orientation");
        assert_eq!(Language::Ja.tr("Page Layout|Orientation"), "印刷の向き");
        assert_eq!(Language::Ja.tr("Orientation"), "方向");
        assert_eq!(Language::Cs.tr("Page Layout|Orientation"), "Orientace");
        assert_eq!(Language::Cs.tr("Orientation"), "Orientace textu");
        assert_eq!(Language::Sr.tr("Data"), "Podaci");
        assert_eq!(Language::Sr.tr("Sheet1!A1"), "Sheet1!A1");
        assert_eq!(Language::Sr.tr("Page Layout|Orientation"), "Orijentacija");
        assert_eq!(Language::Sr.tr("Orientation"), "Orijentacija");
        assert_eq!(Language::parse("xx"), None);
    }

    #[test]
    fn unknown_saved_language_keeps_the_other_preferences() {
        let ui: crate::UiState = serde_json::from_str(r#"{"dark": true, "language": "xx"}"#).unwrap();
        assert!(ui.dark);
        assert_eq!(ui.language, Language::En);
        let ui: crate::UiState = serde_json::from_str(r#"{"dark": true, "language": 7}"#).unwrap();
        assert!(ui.dark);
        let ui: crate::UiState = serde_json::from_str(r#"{"language": "ja"}"#).unwrap();
        assert_eq!(ui.language, Language::Ja);
        assert_eq!(crate::UiState::default().language, Language::En, "the default never reads the host locale");
        assert_eq!(saved_language(&serde_json::json!({"language": "ja"})), Some(Language::Ja));
        assert_eq!(saved_language(&serde_json::json!({"language": "xx"})), None);
        assert_eq!(saved_language(&serde_json::json!({})), None);
    }

    #[test]
    fn tags_reduce_to_a_supported_language() {
        assert_eq!(Language::from_tag("ja-JP"), Some(Language::Ja));
        assert_eq!(Language::from_tag("en_US"), Some(Language::En));
        assert_eq!(Language::from_tag("es_MX"), Some(Language::Es));
        assert_eq!(Language::from_tag("zh-Hans-CN"), Some(Language::Zh));
        assert_eq!(Language::from_tag("ko-KR"), Some(Language::Ko));
        assert_eq!(Language::from_tag("RU_ru"), Some(Language::Ru));
        assert_eq!(Language::from_tag("de-DE"), Some(Language::De));
        assert_eq!(Language::from_tag("de_AT"), Some(Language::De));
        assert_eq!(Language::from_tag("de-CH"), Some(Language::De));
        assert_eq!(Language::from_tag("pt-BR"), Some(Language::PtBr));
        assert_eq!(Language::parse("pt"), Some(Language::PtBr));
        assert_eq!(Language::parse("pt-br"), Some(Language::PtBr));
        assert_eq!(Language::from_tag("cs-CZ"), Some(Language::Cs));
        assert_eq!(Language::from_tag("cs_CZ.UTF-8"), Some(Language::Cs));
        assert_eq!(Language::parse("cs"), Some(Language::Cs));
        assert_eq!(Language::from_tag("fr-FR"), Some(Language::Fr));
        assert_eq!(Language::from_tag("fr_CA"), Some(Language::Fr));
        assert_eq!(Language::parse("fr"), Some(Language::Fr));
        assert_eq!(Language::from_tag("sr-Latn-RS"), Some(Language::Sr));
        assert_eq!(Language::from_tag("sr-RS"), Some(Language::Sr));
        assert_eq!(Language::parse("sr"), Some(Language::Sr));
    }

    #[test]
    fn an_unset_language_preference_is_english_until_the_app_applies_the_system_one() {
        // Deserializing never reads the host locale (tests stay deterministic); the desktop app
        // applies `Language::system()` itself when `ui.json` names no usable language.
        let restored: crate::UiState = serde_json::from_str("{}").unwrap();
        assert_eq!(restored.language, Language::En);
        assert_eq!(saved_language(&serde_json::json!({"language": "zh"})), Some(Language::Zh));
    }

    #[test]
    fn language_commands_switch_and_persist() {
        let mut app = crate::SheetApp::new(gridcraft_engine::Session::default(), Default::default());
        for language in Language::ALL {
            app.run("app.language.set", serde_json::json!({"language": language.code()})).unwrap();
            assert_eq!(app.ui.language, language);
            let restored: crate::UiState = serde_json::from_str(&serde_json::to_string(&app.ui).unwrap()).unwrap();
            assert_eq!(restored.language, language);
        }
        app.run("app.language.set", serde_json::json!({"code": "ja"})).unwrap();
        assert_eq!(app.ui.language, Language::Ja, "`code` is an alias for `language`");
        assert!(app.run("app.language.set", serde_json::json!({})).is_err(), "no language is an error, not a dialog");
        app.run("app.language.english", serde_json::json!({})).unwrap();
        assert_eq!(app.ui.language, Language::En);
        for l in Language::ALL {
            assert_eq!(Language::parse(l.code()), Some(l));
        }
    }
}
