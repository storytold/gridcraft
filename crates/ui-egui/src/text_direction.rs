//! Cell reading direction follows OOXML context rules, independently of sheet geometry.

use egui::epaint::text::TextDirection;
use gridcraft_engine::model::ReadingOrder;

pub(crate) fn cell_direction(order: Option<ReadingOrder>, text: &str) -> TextDirection {
    match order.unwrap_or_default() {
        ReadingOrder::LeftToRight => TextDirection::LeftToRight,
        ReadingOrder::RightToLeft => TextDirection::RightToLeft,
        ReadingOrder::Context => match text.chars().find(|ch| !ch.is_whitespace()).map(unicode_bidi::bidi_class) {
            Some(unicode_bidi::BidiClass::R | unicode_bidi::BidiClass::AL) => TextDirection::RightToLeft,
            _ => TextDirection::LeftToRight,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_uses_the_first_non_whitespace_character() {
        for text in ["مرحبا 123", "  עברית", "\n\tالعربية"] {
            assert_eq!(cell_direction(None, text), TextDirection::RightToLeft);
        }
        for text in ["123 مرحبا", "(مرحبا)", "ABC العربية", "", "   "] {
            assert_eq!(cell_direction(None, text), TextDirection::LeftToRight);
        }
        assert_eq!(cell_direction(Some(ReadingOrder::LeftToRight), "مرحبا"), TextDirection::LeftToRight);
        assert_eq!(cell_direction(Some(ReadingOrder::RightToLeft), "123 ABC"), TextDirection::RightToLeft);
    }
}
