# 引擎錯誤訊息。索引鍵：error-<代碼>；代碼對應 gridcraft_engine::EngineError。
# GridCraft 原創用語（繁體中文，臺灣）；非摘自第三方文件。

error-unknown-command = 不明的命令「{ $command }」。
error-command-disabled = 「{ $command }」目前無法使用：{ $reason }
error-bad-params = 「{ $command }」的參數無效：{ $message }
error-no-document = 目前沒有開啟任何活頁簿。
error-internal = 「{ $command }」發生內部錯誤（活頁簿已維持原狀）：{ $message }
error-other = { $message }
error-invalid-formula = 此公式有問題：{ $message }
error-invalid-locale = 語言或地區設定無效：{ $message }
error-protected-sheet = 不允許此變更：該儲存格或圖表位於受保護的工作表中。
error-encrypted-workbook = 「{ $name }」受密碼保護。請在 Excel 中移除密碼後重試；GridCraft 目前無法開啟加密的活頁簿。
error-legacy-workbook = 「{ $name }」是 Excel 97–2003 (.xls) 活頁簿或其他舊版二進位檔案。請在 Excel 中另存為 .xlsx 後重試。
error-import-failed = 無法匯入「{ $name }」：{ $message }
error-import-only-format = { $format }僅支援資料匯入。請另存為 .xlsx 或其他支援的匯出格式。
error-text-box-too-long = 文字方塊最多可包含 { $limit } 個字元。
error-chart-range-too-large = 來源範圍過大，無法用於圖表。
