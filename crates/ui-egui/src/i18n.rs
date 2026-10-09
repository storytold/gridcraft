//! Interface translations. Command ids and document text stay stable; only interface chrome is
//! translated. Untranslated labels fall back to English, so coverage can grow incrementally.
//!
//! The interface language is a preference ([`crate::UiState::language`]): on first run it defaults
//! to the system's language ([`Language::system`]), and the View tab switches it by hand.
//!
//! No Japanese font is bundled and none is installed here: the CJK faces [`crate::theme`] appends
//! to every family already cover kana and kanji, so Japanese interface text renders without help.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    En,
    Ja,
    #[serde(rename = "pt")]
    PtBr,
}

impl Language {
    pub const ALL: [Self; 3] = [Self::En, Self::Ja, Self::PtBr];

    /// The language's own name, shown in the switcher.
    pub fn name(self) -> &'static str {
        match self {
            Self::En => "English",
            Self::Ja => "日本語",
            Self::PtBr => "Português (Brasil)",
        }
    }

    pub fn parse(code: &str) -> Option<Self> {
        match code {
            "en" => Some(Self::En),
            "ja" => Some(Self::Ja),
            "pt" | "pt-br" => Some(Self::PtBr),
            _ => None,
        }
    }

    /// A language tag (`ja`, `ja-JP`, `en_US`) reduced to a supported language. Native only: the web
    /// build has no system locale to read and tests exercise it on the host.
    #[cfg(not(target_arch = "wasm32"))]
    fn from_tag(tag: &str) -> Option<Self> {
        Self::parse(&tag.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase())
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
            Self::En
        }
    }

    pub fn tr(self, text: &str) -> &str {
        let table: &[(&str, &str)] = match self {
            Self::En => &[],
            Self::Ja => JAPANESE,
            Self::PtBr => PORTUGUESE,
        };
        table.iter().find(|(english, _)| *english == text).map_or(text, |(_, translated)| translated)
    }
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
    ("Orientation", "印刷の向き"),
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
    ("Hide Rows", "行の表示/非表示"),
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
    ("Header & Footer", "ヘッダーとフッター"),
    ("Draw with ink", "インクで描画"),
    ("Eraser", "消しゴム"),
    ("Ink to\nShape", "インクを図形に変換"),
    ("Select\nObjects", "オブジェクトの選択"),
    ("Themes", "テーマ"),
    ("Margins", "余白"),
    ("Size", "サイズ"),
    ("Print\nArea", "印刷範囲"),
    ("Set Print Area", "印刷範囲の設定"),
    ("Breaks", "改ページ"),
    ("Insert Page Break", "改ページの挿入"),
    ("Remove Page Break", "改ページの解除"),
    ("Reset All Page Breaks", "すべての改ページを解除"),
    ("Print\nTitles", "印刷タイトル"),
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
    ("Trace Dependents", "依存元のトレース"),
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
    ("Subtotal", "小計"),
    ("What-If\nAnalysis", "What-If 分析"),
    ("Data Table…", "データ テーブル…"),
    ("Scenario Manager…", "シナリオ マネージャー…"),
    ("Goal Seek…", "ゴール シーク…"),
    ("Spelling", "スペルチェック"),
    ("Check\nAccessibility", "アクセシビリティ チェック"),
    ("Threaded Comments", "スレッド化されたコメント"),
    ("Show\nComments", "コメントの表示"),
    ("Protect\nWorkbook", "ブックの保護"),
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
    ("Interface language", "表示言語"),
    ("Command\nPalette", "コマンド パレット"),
    ("Agent\nControl", "エージェント制御"),
    ("Action\nJournal", "操作ジャーナル"),
    ("About\nGridCraft", "GridCraft について"),
    ("Share", "共有"),
    ("Add Chart\nElement", "グラフ要素を追加"),
    ("Change\nChart Type", "グラフの種類の変更"),
    ("Switch\nRow/Column", "行/列の切り替え"),
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
];

/// English label → Brazilian Portuguese, keyed like [`JAPANESE`] so the tables stay in step (a test
/// enforces it) and the diff reads against the Japanese column. Labels follow Excel's pt-BR wording;
/// anything absent falls back to English.
const PORTUGUESE: &[(&str, &str)] = &[
    ("Paste", "Colar"),
    ("Paste Values", "Colar Valores"),
    ("Paste Formulas", "Colar Fórmulas"),
    ("Paste Formatting", "Colar Formatação"),
    ("Transpose", "Transpor"),
    ("Paste Link", "Colar Vínculo"),
    ("Paste Special…", "Colar Especial…"),
    ("Cut (⌘X)", "Recortar (⌘X)"),
    ("Copy (⌘C)", "Copiar (⌘C)"),
    ("Format Painter (double-click to keep it on)", "Pincel de Formatação (clique duas vezes para mantê-lo ativo)"),
    ("Bold (⌘B)", "Negrito (⌘B)"),
    ("Italic (⌘I)", "Itálico (⌘I)"),
    ("Underline (⌘U)", "Sublinhado (⌘U)"),
    ("Double Underline", "Sublinhado Duplo"),
    ("Strikethrough", "Riscado"),
    ("Underline", "Sublinhado"),
    ("Increase Font Size", "Aumentar Tamanho da Fonte"),
    ("Decrease Font Size", "Diminuir Tamanho da Fonte"),
    ("Wrap Text", "Quebrar Texto Automaticamente"),
    ("Merge & Center", "Mesclar e Centralizar"),
    ("Merge Cells", "Mesclar Células"),
    ("Merge Across", "Mesclar Através"),
    ("Unmerge Cells", "Desmesclar Células"),
    ("Fill Color", "Cor de Preenchimento"),
    ("Font Color", "Cor da Fonte"),
    ("Borders", "Bordas"),
    ("Top\nAlign", "Alinhar\nno Topo"),
    ("Middle\nAlign", "Alinhar\nno Meio"),
    ("Bottom\nAlign", "Alinhar\nem Baixo"),
    ("Left", "Esquerda"),
    ("Right", "Direita"),
    ("Center", "Centro"),
    ("Horizontal", "Horizontal"),
    ("Vertical Text", "Texto Vertical"),
    ("Decrease Indent", "Diminuir Recuo"),
    ("Increase Indent", "Aumentar Recuo"),
    ("Orientation", "Orientação"),
    ("Angle Counterclockwise", "Ângulo Anti-Horário"),
    ("Angle Clockwise", "Ângulo Horário"),
    ("Rotate Text Up", "Girar Texto para Cima"),
    ("Rotate Text Down", "Girar Texto para Baixo"),
    ("Accounting Number Format", "Formato de Número Contábil"),
    ("Percent Style", "Estilo de Porcentagem"),
    ("Comma Style", "Estilo de Vírgula"),
    ("Increase Decimal", "Aumentar Casas Decimais"),
    ("Decrease Decimal", "Diminuir Casas Decimais"),
    ("More Accounting Formats…", "Mais Formatos Contábeis…"),
    ("Conditional\nFormatting", "Formatação\nCondicional"),
    ("Conditional Formatting", "Formatação Condicional"),
    ("Format\nas Table", "Formatar\ncomo Tabela"),
    ("Cell\nStyles", "Estilos\nde Célula"),
    ("Format Cells…", "Formatar Células…"),
    ("Format Cell Alignment…", "Formatar Alinhamento de Células…"),
    ("Format\nPane", "Painel de\nFormatação"),
    ("Format", "Formatar"),
    ("Insert", "Inserir"),
    ("Insert Cells…", "Inserir Células…"),
    ("Delete", "Excluir"),
    ("Delete Cells…", "Excluir Células…"),
    ("Insert Sheet Rows", "Inserir Linhas de Planilha"),
    ("Insert Sheet Columns", "Inserir Colunas de Planilha"),
    ("Delete Sheet Rows", "Excluir Linhas de Planilha"),
    ("Delete Sheet Columns", "Excluir Colunas de Planilha"),
    ("Row Height…", "Altura da Linha…"),
    ("AutoFit Row Height", "Ajustar Altura da Linha"),
    ("Column Width…", "Largura da Coluna…"),
    ("AutoFit Column Width", "Ajustar Largura da Coluna"),
    ("Default Width…", "Largura Padrão…"),
    ("Hide Rows", "Ocultar Linhas"),
    ("Unhide Rows", "Reexibir Linhas"),
    ("Hide Columns", "Ocultar Colunas"),
    ("Unhide Columns", "Reexibir Colunas"),
    ("Lock Cell", "Bloquear Célula"),
    ("Clear", "Limpar"),
    ("Clear All", "Limpar Tudo"),
    ("Clear Formats", "Limpar Formatos"),
    ("Clear Contents", "Limpar Conteúdo"),
    ("Clear Comments and Notes", "Limpar Comentários e Notas"),
    ("Clear Hyperlinks", "Limpar Hiperlinks"),
    ("AutoSum", "Soma Automática"),
    ("AutoSum (⌘⇧T)", "Soma Automática (⌘⇧T)"),
    ("Flash\nFill", "Preenchimento\nRelâmpago"),
    ("Flash Fill", "Preenchimento Relâmpago"),
    ("Fill", "Preencher"),
    ("Clear Print Area", "Limpar Área de Impressão"),
    ("Find &\nSelect", "Localizar e\nSelecionar"),
    ("Find…", "Localizar…"),
    ("Replace…", "Substituir…"),
    ("Go To…", "Ir Para…"),
    ("Go To Special…", "Ir Para Especial…"),
    ("PivotTable", "Tabela Dinâmica"),
    ("Recommended\nCharts", "Gráficos\nRecomendados"),
    ("Table", "Tabela"),
    ("Pictures", "Imagens"),
    ("Shapes", "Formas"),
    ("Icons", "Ícones"),
    ("Text\nBox", "Caixa\nde Texto"),
    ("Comment", "Comentário"),
    ("New\nComment", "Novo\nComentário"),
    ("New Note", "Nova Nota"),
    ("Show/Hide Note", "Mostrar/Ocultar Nota"),
    ("Header &\nFooter", "Cabeçalho e\nRodapé"),
    ("Text to\nColumns", "Texto para\nColunas"),
    ("Link", "Vínculo"),
    ("Symbol", "Símbolo"),
    ("Sparklines", "Minigráficos"),
    ("Header & Footer", "Cabeçalho e Rodapé"),
    ("Draw with ink", "Desenhar com Tinta"),
    ("Eraser", "Borracha"),
    ("Ink to\nShape", "Tinta para\nForma"),
    ("Select\nObjects", "Selecionar\nObjetos"),
    ("Themes", "Temas"),
    ("Margins", "Margens"),
    ("Size", "Tamanho"),
    ("Print\nArea", "Área de\nImpressão"),
    ("Set Print Area", "Definir Área de Impressão"),
    ("Breaks", "Quebras"),
    ("Insert Page Break", "Inserir Quebra de Página"),
    ("Remove Page Break", "Remover Quebra de Página"),
    ("Reset All Page Breaks", "Redefinir Todas as Quebras de Página"),
    ("Print\nTitles", "Imprimir\nTítulos"),
    ("Narrow", "Estreita"),
    ("Wide", "Ampla"),
    ("Normal", "Normal"),
    ("Portrait", "Retrato"),
    ("Landscape", "Paisagem"),
    ("Custom Margins…", "Margens Personalizadas…"),
    ("Insert\nFunction", "Inserir\nFunção"),
    ("Name\nManager", "Gerenciador\nde Nomes"),
    ("Define Name", "Definir Nome"),
    ("Use in Formula", "Usar na Fórmula"),
    ("Create from Selection", "Criar a Partir da Seleção"),
    ("Trace Precedents", "Rastrear Precedentes"),
    ("Trace Dependents", "Rastrear Dependentes"),
    ("Remove Arrows", "Remover Setas"),
    ("Show Formulas", "Exibir Fórmulas"),
    ("Error Checking", "Verificação de Erros"),
    ("Evaluate Formula", "Avaliar Fórmula"),
    ("Watch Window", "Janela de Inspeção"),
    ("Calculation\nOptions", "Opções de\nCálculo"),
    ("Calculate Now", "Calcular Agora"),
    ("Calculate Sheet", "Calcular Planilha"),
    ("Automatic", "Automático"),
    ("Automatic Except for Data Tables", "Automático Exceto Tabelas de Dados"),
    ("Manual", "Manual"),
    ("Sort &\nFilter", "Classificar e\nFiltrar"),
    ("Sort", "Classificar"),
    ("Sort A to Z", "Classificar de A a Z"),
    ("Sort Z to A", "Classificar de Z a A"),
    ("Custom Sort…", "Classificação Personalizada…"),
    ("Filter", "Filtrar"),
    ("Reapply", "Aplicar Novamente"),
    ("Get Data\n(Text/CSV)", "Obter Dados\n(Texto/CSV)"),
    ("Remove\nDuplicates", "Remover\nDuplicatas"),
    ("Data\nValidation", "Validação\nde Dados"),
    ("Data Validation", "Validação de Dados"),
    ("Data Validation…", "Validação de Dados…"),
    ("Circle Invalid Data", "Circundar Dados Inválidos"),
    ("Clear Validation", "Limpar Validação"),
    ("Group", "Agrupar"),
    ("Ungroup", "Desagrupar"),
    ("Subtotal", "Subtotal"),
    ("What-If\nAnalysis", "Análise de\nHipóteses"),
    ("Data Table…", "Tabela de Dados…"),
    ("Scenario Manager…", "Gerenciador de Cenários…"),
    ("Goal Seek…", "Buscar Objetivo…"),
    ("Spelling", "Ortografia"),
    ("Check\nAccessibility", "Verificar\nAcessibilidade"),
    ("Threaded Comments", "Comentários Encadeados"),
    ("Show\nComments", "Mostrar\nComentários"),
    ("Protect\nWorkbook", "Proteger\nPasta de Trabalho"),
    ("Unprotect\nSheet", "Desproteger\nPlanilha"),
    ("Protect Sheet…", "Proteger Planilha…"),
    ("Comments", "Comentários"),
    ("Notes", "Notas"),
    ("Page Break\nPreview", "Visualização\nde Quebras de Página"),
    ("Page\nLayout", "Layout\nde Página"),
    ("Formula Bar", "Barra de Fórmulas"),
    ("Gridlines", "Linhas de Grade"),
    ("Headings", "Títulos"),
    ("Zoom", "Zoom"),
    ("100%", "100%"),
    ("Zoom to\nSelection", "Zoom na\nSeleção"),
    ("Freeze\nPanes", "Congelar\nPainéis"),
    ("Freeze Panes", "Congelar Painéis"),
    ("Freeze Top Row", "Congelar Linha Superior"),
    ("Freeze First Column", "Congelar Primeira Coluna"),
    ("Unfreeze Panes", "Descongelar Painéis"),
    ("Dark Mode", "Modo Escuro"),
    ("Interface language", "Idioma da interface"),
    ("Command\nPalette", "Paleta de\nComandos"),
    ("Agent\nControl", "Controle\ndo Agente"),
    ("Action\nJournal", "Registro\nde Ações"),
    ("About\nGridCraft", "Sobre o\nGridCraft"),
    ("Share", "Compartilhar"),
    ("Add Chart\nElement", "Adicionar\nElemento de Gráfico"),
    ("Change\nChart Type", "Alterar\nTipo de Gráfico"),
    ("Switch\nRow/Column", "Alternar\nLinha/Coluna"),
    ("Data Labels: None", "Rótulos de Dados: Nenhum"),
    ("Data Labels: Show", "Rótulos de Dados: Exibir"),
    ("Legend: None", "Legenda: Nenhuma"),
    ("Legend: Bottom", "Legenda: Abaixo"),
    ("Legend: Top", "Legenda: Acima"),
    ("Legend: Right", "Legenda: À Direita"),
    ("Gridlines: None", "Linhas de Grade: Nenhuma"),
    ("Gridlines: Show", "Linhas de Grade: Exibir"),
    ("Chart\nTitle", "Título\ndo Gráfico"),
    ("Delete\nChart", "Excluir\nGráfico"),
    ("Convert\nto Range", "Converter\nem Intervalo"),
    ("Table\nStyles", "Estilos\nde Tabela"),
    ("Top 10 Items", "10 Primeiros Itens"),
    ("Top 10%", "10% Primeiros"),
    ("Bottom 10 Items", "10 Últimos Itens"),
    ("Bottom 10%", "10% Últimos"),
    ("Above Average", "Acima da Média"),
    ("Below Average", "Abaixo da Média"),
    ("Clear Rules from Entire Sheet", "Limpar Regras de Toda a Planilha"),
    ("Clear Rules from Selected Cells", "Limpar Regras das Células Selecionadas"),
    ("Constants", "Constantes"),
    ("Data Bars", "Barras de Dados"),
    ("Color Scales", "Escalas de Cores"),
    ("Icon Sets", "Conjuntos de Ícones"),
    ("Home", "Página Inicial"),
    ("Draw", "Desenhar"),
    ("Page Layout", "Layout da Página"),
    ("Formulas", "Fórmulas"),
    ("Data", "Dados"),
    ("Review", "Revisão"),
    ("View", "Exibir"),
    ("Automate", "Automatizar"),
    ("Table Design", "Design de Tabela"),
    ("Chart Design", "Design de Gráfico"),
    ("Ready", "Pronto"),
    ("Enter", "Inserir"),
    ("Edit", "Editar"),
    ("Point", "Apontar"),
    ("Calculate", "Calcular"),
    ("Select destination and press Enter or choose Paste", "Selecione o destino e pressione Enter ou escolha Colar"),
    ("General", "Geral"),
    ("Number", "Número"),
    ("Currency", "Moeda"),
    ("Accounting", "Contábil"),
    ("Date", "Data"),
    ("Time", "Hora"),
    ("Percentage", "Porcentagem"),
    ("Fraction", "Fração"),
    ("Scientific", "Científico"),
    ("Text", "Texto"),
    ("Special", "Especial"),
    ("Custom", "Personalizado"),
    ("Short Date", "Data Abreviada"),
    ("Long Date", "Data Por Extenso"),
    ("More Number Formats…", "Mais Formatos de Número…"),
    ("Financial", "Financeira"),
    ("Logical", "Lógica"),
    ("Date &\nTime", "Data e\nHora"),
    ("Lookup &\nReference", "Pesquisa e\nReferência"),
    ("Math &\nTrig", "Matemática e\nTrigonometria"),
    ("More\nFunctions", "Mais\nFunções"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translations_are_unique_and_preserve_unknown_text() {
        for (i, (en, ja)) in JAPANESE.iter().enumerate() {
            assert!(!ja.is_empty());
            assert!(JAPANESE.iter().take(i).all(|(other, _)| en != other));
            assert_eq!(Language::En.tr(en), *en);
        }
        for (i, (en, pt)) in PORTUGUESE.iter().enumerate() {
            assert!(!pt.is_empty());
            assert!(PORTUGUESE.iter().take(i).all(|(other, _)| en != other));
            assert_eq!(Language::En.tr(en), *en);
        }
        // The tables cover the same interface strings, so per-language coverage stays even.
        for (en, _) in PORTUGUESE {
            assert!(JAPANESE.iter().any(|(other, _)| en == other), "{en:?} missing from JAPANESE");
        }
        assert_eq!(JAPANESE.len(), PORTUGUESE.len());
        assert_eq!(Language::Ja.tr("Data"), "データ");
        assert_eq!(Language::PtBr.tr("Data"), "Dados");
        assert_eq!(Language::PtBr.tr("Date"), "Data");
        assert_eq!(Language::Ja.tr("Sheet1!A1"), "Sheet1!A1");
        assert_eq!(Language::PtBr.tr("Sheet1!A1"), "Sheet1!A1");
        assert_eq!(Language::parse("xx"), None);
    }

    #[test]
    fn tags_reduce_to_a_supported_language() {
        assert_eq!(Language::from_tag("ja-JP"), Some(Language::Ja));
        assert_eq!(Language::from_tag("en_US"), Some(Language::En));
        assert_eq!(Language::from_tag("de-DE"), None);
        assert_eq!(Language::from_tag("pt-BR"), Some(Language::PtBr));
        assert_eq!(Language::parse("pt"), Some(Language::PtBr));
        assert_eq!(Language::parse("pt-br"), Some(Language::PtBr));
    }

    #[test]
    fn language_commands_switch_and_persist() {
        let mut app = crate::SheetApp::new(gridcraft_engine::Session::default(), Default::default());
        app.run("app.language.japanese", serde_json::json!({})).unwrap();
        assert_eq!(app.ui.language, Language::Ja);
        app.run("app.language.portuguese", serde_json::json!({})).unwrap();
        assert_eq!(app.ui.language, Language::PtBr);
        let restored: crate::UiState = serde_json::from_str(&serde_json::to_string(&app.ui).unwrap()).unwrap();
        assert_eq!(restored.language, Language::PtBr);
        app.run("app.language.english", serde_json::json!({})).unwrap();
        assert_eq!(app.ui.language, Language::En);
    }
}
