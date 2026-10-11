# 引擎错误消息。键：error-<code>；code 与 gridcraft_engine::EngineError 一一对应。
# 以下为 GridCraft 原创的简体中文表述，译自 en-US 参考文本，并非摘自第三方文档。

error-unknown-command = 未知命令“{ $command }”。
error-command-disabled = 当前无法使用“{ $command }”：{ $reason }
error-bad-params = “{ $command }”的参数无效：{ $message }
error-no-document = 没有打开任何工作簿。
error-internal = “{ $command }”发生内部错误（工作簿保持原样）：{ $message }
error-other = { $message }
error-invalid-formula = 此公式存在问题：{ $message }
error-invalid-locale = 语言或区域设置无效：{ $message }
error-protected-sheet = 不允许此更改：该单元格或图表位于受保护的工作表中。
error-encrypted-workbook = “{ $name }”受密码保护。请在 Excel 中删除密码后重试；GridCraft 暂时无法打开加密的工作簿。
error-legacy-workbook = “{ $name }”是 Excel 97–2003 (.xls) 工作簿或其他旧版二进制文件。请在 Excel 中另存为 .xlsx 后重试。
error-import-failed = 无法导入“{ $name }”：{ $message }
error-import-only-format = { $format }仅支持数据导入。请另存为 .xlsx 或其他受支持的导出格式。
error-text-box-too-long = 文本框最多可包含 { $limit } 个字符。
error-chart-range-too-large = 源区域过大，无法用于图表。
