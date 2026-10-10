# Engine error messages. Key: error-<code>; the codes mirror gridcraft_engine::EngineError.
# Original wording for GridCraft, translated from the en-US reference.

error-unknown-command = コマンド「{ $command }」は不明です。
error-command-disabled = 「{ $command }」は現在使用できません: { $reason }
error-bad-params = 「{ $command }」のパラメーターが正しくありません: { $message }
error-no-document = 開いているブックはありません。
error-internal = 「{ $command }」で内部エラーが発生しました (ブックは元の状態のままです): { $message }
error-other = { $message }
error-invalid-formula = この数式には問題があります: { $message }
error-invalid-locale = 言語または地域の設定が正しくありません: { $message }
error-protected-sheet = この変更はできません: セルまたはグラフが保護されたシート上にあります。
error-encrypted-workbook = 「{ $name }」はパスワードで保護されています。Excel でパスワードを解除してから再試行してください。GridCraft は暗号化されたブックにまだ対応していません。
error-legacy-workbook = 「{ $name }」は Excel 97–2003 (.xls) のブックまたは旧形式のバイナリファイルです。Excel で .xlsx 形式に保存してから再試行してください。
error-import-failed = 「{ $name }」をインポートできません: { $message }
error-import-only-format = { $format } はデータのインポートのみ対応しています。.xlsx または対応する別のエクスポート形式で保存してください。
error-text-box-too-long = テキストボックスに入力できる文字数は最大 { $limit } 文字です。
error-chart-range-too-large = グラフの元データの範囲が大きすぎます。
