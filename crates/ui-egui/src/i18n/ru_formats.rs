//! Russian custom-format spellings. Token names follow Microsoft's public format-code guide:
//! https://support.microsoft.com/ru-ru/excel/format-a-date-the-way-you-want-in-excel

pub(super) const FORMAT: crate::format_locale::FormatLocale = crate::format_locale::FormatLocale {
    tokens: &[('d', 'д'), ('m', 'м'), ('y', 'г'), ('h', 'ч'), ('s', 'с')],
    colors: &[
        ("Black", "Черный"),
        ("Blue", "Синий"),
        ("Cyan", "Голубой"),
        ("Green", "Зеленый"),
        ("Magenta", "Пурпурный"),
        ("Red", "Красный"),
        ("White", "Белый"),
        ("Yellow", "Желтый"),
    ],
    general: "Общий",
    short_date: Some("dd.mm.yyyy"),
    time: Some("hh:mm:ss"),
};
