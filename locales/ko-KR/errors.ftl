# 엔진 오류 메시지. 키: error-<코드>; 코드는 gridcraft_engine::EngineError와 대응합니다.
# GridCraft 원문을 한국어로 옮긴 독자적 문구이며, 제3자 문서에서 가져온 것이 아닙니다.

error-unknown-command = 알 수 없는 명령입니다: “{ $command }”
error-command-disabled = “{ $command }” 명령은 지금 사용할 수 없습니다: { $reason }
error-bad-params = “{ $command }” 명령의 매개 변수가 올바르지 않습니다: { $message }
error-no-document = 열려 있는 통합 문서가 없습니다.
error-internal = “{ $command }” 명령에서 내부 오류가 발생했습니다(통합 문서는 그대로 유지되었습니다): { $message }
error-other = { $message }
error-invalid-formula = 이 수식에 문제가 있습니다: { $message }
error-invalid-locale = 언어 또는 지역 설정이 올바르지 않습니다: { $message }
error-protected-sheet = 이 변경은 허용되지 않습니다. 셀 또는 차트가 보호된 시트에 있습니다.
error-encrypted-workbook = “{ $name }” 파일은 암호로 보호되어 있습니다. Excel에서 암호를 제거한 후 다시 시도하세요. GridCraft는 아직 암호화된 통합 문서를 열 수 없습니다.
error-legacy-workbook = “{ $name }” 파일은 Excel 97–2003(.xls) 통합 문서 또는 다른 이전 바이너리 파일입니다. Excel에서 .xlsx로 저장한 후 다시 시도하세요.
error-import-failed = “{ $name }” 파일을 가져올 수 없습니다: { $message }
error-import-only-format = { $format } 형식은 데이터 가져오기만 지원합니다. .xlsx 또는 다른 지원되는 내보내기 형식으로 저장하세요.
error-text-box-too-long = 텍스트 상자에는 최대 { $limit }자를 입력할 수 있습니다.
error-chart-range-too-large = 원본 범위가 너무 커서 차트를 만들 수 없습니다.
