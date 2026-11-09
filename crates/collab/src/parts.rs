//! Splitting one `applied` into frames (ADR-0013).
//!
//! A structural undo or redo carries the whole sheets of its step, and the workbook-level state of
//! the same boundary (defined names, structure protection). One image can be far larger than the
//! frame the transport writes, so the image travels as parts of its serialised form and a replica
//! applies the operation only once the last part is in.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Serialize;

use crate::protocol::{Applied, ImagePart, SheetImage};

/// Bytes of the serialised image in one part, before base64. A part of 512 KiB becomes about
/// 683 KiB encoded, so the JSON frame stays under the 1 MiB frame limit of the wire (RFC §4.1).
pub const IMAGE_PART: usize = 512 * 1024;

/// The payload of one chunked image, borrowed from the operation so a large image is not cloned
/// just to be cut. Its field names are those of [`ImageBundle`], which the client reads back.
#[derive(Serialize)]
struct BundleRef<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    book: Option<&'a gridcraft_model::WorkbookMeta>,
    images: &'a [SheetImage],
}

/// The frames one operation goes out in: itself while its images fit a part, otherwise a
/// sequence of [`ImagePart`]s.
///
/// ADR-0013 fixes the contract around the sequence: `version`, `epoch` and `opId` repeat on every
/// part, the parts of one operation are written back to back, a client applies the operation only
/// after `more:false`, and a connection that breaks in the middle is caught up from a version
/// before the first part. This function only cuts; the ordering is the caller's queue.
pub fn split(applied: Applied, budget: usize) -> Vec<Applied> {
    let Some(images) = &applied.images else { return vec![applied] };
    let bundle = BundleRef { book: applied.book.as_deref(), images };
    let Ok(bytes) = serde_json::to_vec(&bundle) else { return vec![applied] };
    let budget = budget.max(1);
    if bytes.len() <= budget {
        return vec![applied];
    }
    let total = bytes.len().div_ceil(budget);
    let Ok(total) = u32::try_from(total) else { return vec![applied] };
    let mut frames = Vec::with_capacity(total as usize);
    for seq in 0..total {
        let start = seq as usize * budget;
        let end = ((seq as usize + 1) * budget).min(bytes.len());
        let mut part = applied.clone();
        part.images = None;
        part.book = None;
        part.image_part = Some(ImagePart { seq, total, data: STANDARD.encode(&bytes[start..end]) });
        part.more = seq + 1 < total;
        frames.push(part);
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ImageBundle, Outcome, SheetName};
    use gridcraft_engine::Session;
    use serde_json::json;

    /// A sheet whose image is over `bytes` long: one cell holding a string of that size.
    fn big_sheet(bytes: usize) -> SheetImage {
        let mut session = Session::new();
        session.new_workbook();
        session.execute("cell.set", json!({"cell": "A1", "input": "x".repeat(bytes)})).expect("the cell takes the string");
        let doc = session.doc().expect("a document");
        let sheet = doc.wb.active().expect("a sheet").clone();
        SheetImage { key: "0".into(), name: sheet.name.clone(), index: 0, sheet, preserved: None }
    }

    fn applied_with(images: Vec<SheetImage>) -> Applied {
        Applied {
            op_id: "undo-op-1".into(),
            version: 7,
            epoch: 3,
            actor: "ann".into(),
            result: Outcome { command: None, delta: None },
            sheets: Some(vec![SheetName { key: "0".into(), name: "Sheet1".into() }]),
            images: Some(images),
            book: None,
            more: false,
            image_part: None,
        }
    }

    #[test]
    fn a_small_image_travels_in_one_frame() {
        let applied = applied_with(vec![big_sheet(1024)]);
        let frames = split(applied.clone(), IMAGE_PART);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0], applied);
    }

    #[test]
    fn an_eight_megabyte_image_travels_as_parts_that_stay_inside_one_frame() {
        // Over 8 MiB, the limit that used to refuse the operation as `too_large`.
        let applied = applied_with(vec![big_sheet(9 * 1024 * 1024)]);
        let frames = split(applied.clone(), IMAGE_PART);
        assert!(frames.len() >= 2, "the image does not fit one part");
        let total = u32::try_from(frames.len()).expect("a countable number of parts");

        let mut reassembled = Vec::new();
        for (index, frame) in frames.iter().enumerate() {
            assert!(frame.images.is_none(), "a part carries no typed image");
            assert!(frame.book.is_none(), "the book travels in the parts, not beside them");
            assert_eq!(frame.op_id, applied.op_id, "the opId repeats on every part");
            assert_eq!(frame.version, applied.version, "the version repeats on every part");
            assert_eq!(frame.epoch, applied.epoch, "the epoch repeats on every part");
            assert_eq!(frame.sheets, applied.sheets, "the sheet map rides along");
            let part = frame.image_part.as_ref().expect("a part");
            assert_eq!(part.seq as usize, index);
            assert_eq!(part.total, total);
            assert_eq!(frame.more, index + 1 < frames.len(), "more is true until the last part");
            let encoded = serde_json::to_string(&crate::ServerMessage::Applied { applied: frame.clone() }).expect("a frame is JSON");
            assert!(encoded.len() <= 1024 * 1024, "part {} is {} bytes, over one frame", index, encoded.len());
            reassembled.extend(STANDARD.decode(&part.data).expect("a part is base64"));
        }

        let bundle: ImageBundle = serde_json::from_slice(&reassembled).expect("the parts rebuild the image");
        assert_eq!(bundle.images.len(), 1);
        assert_eq!(bundle.images[0].name, "Sheet1");
        // The rebuilt image is byte-for-byte what the room put in.
        assert_eq!(serde_json::to_vec(&bundle.images).expect("JSON"), serde_json::to_vec(&applied.images.expect("the image")).expect("JSON"));
    }
}
