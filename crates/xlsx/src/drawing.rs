//! DrawingML spreadsheet drawings (`xl/drawings/drawingN.xml`): anchors, pictures, shapes and
//! chart frames.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS};
use gridcraft_model::{
    Anchor, AnchorMode, Color, DrawingGroup, DrawingRef, Image, MAX_GROUP_DEPTH, ObjectRect, Shape, ShapeFormat, ShapeKind, ShapeTextAlign,
    ShapeTextAnchor, Sheet, Theme, rotates,
};

use crate::IoError;
use crate::package::Rel;
use crate::read::Ctx;
use crate::tables::EMU_PER_PX;
use crate::xml::{El, esc, esc_attr};

mod ink;

// ---------------------------------------------------------------- reading

fn emu_px(v: Option<i64>) -> f32 {
    v.map(|e| (e as f64 / EMU_PER_PX).clamp(-1e7, 1e7) as f32).unwrap_or(0.0)
}

fn marker(e: &El) -> (CellRef, f32, f32) {
    let num = |n: &str| e.child(n).and_then(|c| c.text.trim().parse::<i64>().ok());
    let col = num("col").unwrap_or(0).clamp(0, MAX_COLS as i64 - 1) as u32;
    let row = num("row").unwrap_or(0).clamp(0, MAX_ROWS as i64 - 1) as u32;
    (CellRef::new(row, col), emu_px(num("colOff")).max(0.0), emu_px(num("rowOff")).max(0.0))
}

fn ext_px(e: Option<&El>) -> (f32, f32) {
    match e {
        Some(x) => (emu_px(x.attr_i64("cx")).max(0.0), emu_px(x.attr_i64("cy")).max(0.0)),
        None => (0.0, 0.0),
    }
}

fn read_anchor(a: &El, sheet: &Sheet) -> Option<Anchor> {
    match a.name.as_str() {
        "twoCellAnchor" => {
            let (cell, dx, dy) = marker(a.child("from")?);
            let (to, tdx, tdy) = a.child("to").map(marker).unwrap_or((cell, dx, dy));
            let x1 = sheet.col_left(cell.col) + dx as f64;
            let y1 = sheet.row_top(cell.row) + dy as f64;
            let x2 = sheet.col_left(to.col) + tdx as f64;
            let y2 = sheet.row_top(to.row) + tdy as f64;
            // Excel stores "Move and size with cells" as a plain twoCellAnchor, "Move but don't
            // size" as editAs="oneCell", and "Don't move or size" as editAs="absolute".
            let mode = match a.attr("editAs") {
                Some("absolute") => AnchorMode::Absolute,
                Some("oneCell") => AnchorMode::MoveOnly,
                _ => AnchorMode::MoveAndSize,
            };
            Some(Anchor { cell, dx, dy, width: (x2 - x1).max(0.0) as f32, height: (y2 - y1).max(0.0) as f32, mode })
        }
        "oneCellAnchor" => {
            let (cell, dx, dy) = marker(a.child("from")?);
            let (width, height) = ext_px(a.child("ext"));
            Some(Anchor { cell, dx, dy, width, height, mode: AnchorMode::MoveOnly })
        }
        "absoluteAnchor" => {
            let pos = a.child("pos");
            let x = emu_px(pos.and_then(|p| p.attr_i64("x"))).max(0.0) as f64;
            let y = emu_px(pos.and_then(|p| p.attr_i64("y"))).max(0.0) as f64;
            let col = sheet.col_at(x);
            let row = sheet.row_at(y);
            let (width, height) = ext_px(a.child("ext"));
            Some(Anchor {
                cell: CellRef::new(row, col),
                dx: (x - sheet.col_left(col)).max(0.0) as f32,
                dy: (y - sheet.row_top(row)).max(0.0) as f32,
                width,
                height,
                mode: AnchorMode::Absolute,
            })
        }
        _ => None,
    }
}

/// Colour of a DrawingML colour choice inside `e` (e.g. a `solidFill`).
pub fn dml_color(e: &El) -> Option<Color> {
    if let Some(c) = e.child("srgbClr").and_then(|c| c.attr("val")) {
        return Color::from_hex(c);
    }
    if let Some(c) = e.child("sysClr").and_then(|c| c.attr("lastClr")) {
        return Color::from_hex(c);
    }
    if let Some(c) = e.child("schemeClr").and_then(|c| c.attr("val")) {
        let i = match c {
            "bg1" | "lt1" => 0,
            "tx1" | "dk1" => 1,
            "bg2" | "lt2" => 2,
            "tx2" | "dk2" => 3,
            "accent1" => 4,
            "accent2" => 5,
            "accent3" => 6,
            "accent4" => 7,
            "accent5" => 8,
            "accent6" => 9,
            "hlink" => 10,
            "folHlink" => 11,
            _ => return None,
        };
        return Some(Color::Theme(i, 0));
    }
    None
}

fn mime_for(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" | "jpe" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "svg" => "image/svg+xml",
        "emf" => "image/x-emf",
        "wmf" => "image/x-wmf",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

/// Where a drawing object goes: the model anchor pictures, charts and shapes take, the pixel box
/// a group lays its children out in, and the enclosing space's scale (pixels per EMU) and flips.
#[derive(Clone, Copy)]
struct Place {
    anchor: Anchor,
    rect: ObjectRect,
    scale: (f64, f64),
    flip: (bool, bool),
}

/// An object's own transform (`a:xfrm`) inside a group: frames carry it directly, groups in
/// `grpSpPr`, pictures and shapes in `spPr`.
fn obj_xfrm(obj: &El) -> Option<&El> {
    match obj.name.as_str() {
        "graphicFrame" => obj.child("xfrm"),
        "grpSp" => obj.path(&["grpSpPr", "xfrm"]),
        _ => obj.path(&["spPr", "xfrm"]),
    }
}

/// A point (`off`/`chOff`, attributes `x`/`y`) or size (`ext`/`chExt`, `cx`/`cy`) of an `a:xfrm`,
/// in EMUs.
fn xfrm_pair(xf: Option<&El>, name: &str, a: &str, b: &str) -> Option<(f64, f64)> {
    let e = xf?.child(name)?;
    Some((e.attr_i64(a).unwrap_or(0) as f64, e.attr_i64(b).unwrap_or(0) as f64))
}

/// The flips (`flipH`, `flipV`) of an `a:xfrm`.
fn xfrm_flip(xf: Option<&El>) -> (bool, bool) {
    xf.map_or((false, false), |x| (x.flag("flipH", false), x.flag("flipV", false)))
}

/// The rotation (`rot`, 60000ths of a degree) of an `a:xfrm`.
fn xfrm_rot(xf: Option<&El>) -> i64 {
    xf.and_then(|x| x.attr_i64("rot")).unwrap_or(0)
}

/// Whether a rotation turns an object about a quarter (45° up to 135°, or 225° up to 315°):
/// Excel then measures it, when sizing its group, by its box swapped about its centre.
fn quarter_turn(rot: i64) -> bool {
    let r = rot.rem_euclid(21_600_000);
    (2_700_000..8_100_000).contains(&r) || (13_500_000..18_900_000).contains(&r)
}

/// The map from a group's child coordinate space (EMUs) onto its on-sheet box: the child box
/// `chOff`/`chExt` of the group's `a:xfrm` fills `rect`, each axis on its own. An axis without a
/// usable child extent takes its scale from the group's `ext`, and failing that (a group of
/// horizontal or vertical lines) from the enclosing space. Children are mirrored about `rect` on
/// the axes the group or an enclosing group flips (ECMA-376 §20.1.7.5).
struct GroupMap {
    rect: ObjectRect,
    origin: (f64, f64),
    scale: (f64, f64),
    flip: (bool, bool),
}

impl GroupMap {
    fn new(xf: Option<&El>, place: Place) -> GroupMap {
        let rect = place.rect;
        let ext = xfrm_pair(xf, "ext", "cx", "cy");
        let ch_ext = xfrm_pair(xf, "chExt", "cx", "cy");
        let origin = xfrm_pair(xf, "chOff", "x", "y").or_else(|| xfrm_pair(xf, "off", "x", "y")).unwrap_or((0.0, 0.0));
        let axis = |len: f64, ch: Option<f64>, ext: Option<f64>, parent: f64| match (ch, ext) {
            (Some(c), _) if c > 0.0 && len > 0.0 => len / c,
            (_, Some(e)) if e > 0.0 => len / e,
            _ => parent,
        };
        let scale =
            (axis(rect.w, ch_ext.map(|s| s.0), ext.map(|s| s.0), place.scale.0), axis(rect.h, ch_ext.map(|s| s.1), ext.map(|s| s.1), place.scale.1));
        let own = xfrm_flip(xf);
        GroupMap { rect, origin, scale, flip: (place.flip.0 != own.0, place.flip.1 != own.1) }
    }

    /// The on-sheet box of a child with transform `xf`; a child without one fills the group. A
    /// child turned a quarter (`turned`) is placed at its turned box: its box swapped about its
    /// centre in the child space, as Excel sizes groups by it.
    fn child(&self, xf: Option<&El>, turned: bool) -> ObjectRect {
        let (Some(mut off), Some(mut ext)) = (xfrm_pair(xf, "off", "x", "y"), xfrm_pair(xf, "ext", "cx", "cy")) else { return self.rect };
        if turned {
            off = (off.0 + (ext.0 - ext.1) / 2.0, off.1 + (ext.1 - ext.0) / 2.0);
            ext = (ext.1, ext.0);
        }
        let mut r = ObjectRect {
            x: self.rect.x + (off.0 - self.origin.0) * self.scale.0,
            y: self.rect.y + (off.1 - self.origin.1) * self.scale.1,
            w: ext.0.max(0.0) * self.scale.0,
            h: ext.1.max(0.0) * self.scale.1,
        };
        if self.flip.0 {
            r.x = 2.0 * self.rect.x + self.rect.w - r.x - r.w;
        }
        if self.flip.1 {
            r.y = 2.0 * self.rect.y + self.rect.h - r.y - r.h;
        }
        r
    }

    /// The child box (offset and extent, EMUs) that the group's on-sheet box maps onto.
    fn child_box(&self) -> ((i64, i64), (i64, i64)) {
        let emu = |v: f64| if v.is_finite() { v.round().clamp(-1e15, 1e15) as i64 } else { 0 };
        ((emu(self.origin.0), emu(self.origin.1)), (emu(self.rect.w / self.scale.0), emu(self.rect.h / self.scale.1)))
    }
}

/// The objects among `children`, resolving markup-compatibility blocks to their first choice
/// holding a frame (chartex charts), else the fallback.
fn drawing_objects(children: &[El]) -> Vec<&El> {
    children
        .iter()
        .flat_map(|o| match o.name.as_str() {
            "AlternateContent" => o
                .child("Choice")
                .filter(|c| c.child("graphicFrame").is_some())
                .or_else(|| o.child("Fallback"))
                .map(|c| c.children.iter().collect())
                .unwrap_or_default(),
            _ => vec![o],
        })
        .collect()
}

pub fn read_drawing(cx: &mut Ctx<'_>, part: &str, sheet: &mut Sheet) -> Result<(), IoError> {
    let Some(x) = cx.optional_xml(part)? else { return Ok(()) };
    let rels = cx.pkg.rels(part)?;
    for a in &x.children {
        let Some(anchor) = read_anchor(a, sheet) else { continue };
        let px = 1.0 / EMU_PER_PX;
        let place = Place { anchor, rect: sheet.anchor_rect(&anchor), scale: (px, px), flip: (false, false) };
        for obj in drawing_objects(&a.children) {
            read_object(cx, &rels, sheet, obj, place, 0)?;
        }
    }
    Ok(())
}

/// Imports one drawing object at `place`, returning what it became. A group's children are laid
/// out through the group's child coordinate space (nested groups compose), keep the outer
/// anchor's mode and are listed, back to front, in a [`DrawingGroup`]; children that aren't
/// supported are dropped with their own warning.
fn read_object(cx: &mut Ctx<'_>, rels: &[Rel], sheet: &mut Sheet, obj: &El, place: Place, depth: usize) -> Result<Option<DrawingRef>, IoError> {
    let anchor = place.anchor;
    match obj.name.as_str() {
        "pic" => {
            let Some(rid) = obj.path(&["blipFill", "blip"]).and_then(|b| b.attr("embed")) else { return Ok(None) };
            let Some(rel) = rels.iter().find(|r| r.id == rid && !r.external) else {
                cx.warn("a linked (not embedded) picture was skipped");
                return Ok(None);
            };
            let target = rel.target.clone();
            let Some(data) = cx.pkg.read(&target)? else {
                cx.warn(format!("missing picture {target}"));
                return Ok(None);
            };
            let alt = obj.path(&["nvPicPr", "cNvPr"]).and_then(|c| c.attr("descr")).unwrap_or("").to_string();
            let id = cx.next_object_id();
            sheet.images.push(Image { id, anchor, data, mime: mime_for(&target).to_string(), alt });
            Ok(Some(DrawingRef::Image(id)))
        }
        "graphicFrame" => {
            let Some(gd) = obj.path(&["graphic", "graphicData"]) else { return Ok(None) };
            let uri = gd.attr("uri").unwrap_or("");
            if let Some(rid) = gd.child("chart").and_then(|c| c.attr("id")) {
                let Some(rel) = rels.iter().find(|r| r.id == rid) else { return Ok(None) };
                let target = rel.target.clone();
                if let Some(cx_xml) = cx.optional_xml(&target)? {
                    let chartex = uri.contains("chartex");
                    match if chartex { crate::chartex::read_chartex(&cx_xml) } else { crate::chart::read_chart(&cx_xml) } {
                        Some(mut ch) => {
                            let id = cx.next_object_id();
                            ch.id = id;
                            ch.anchor = anchor;
                            sheet.charts.push(ch);
                            return Ok(Some(DrawingRef::Chart(id)));
                        }
                        None => cx.warn(format!("chart {target} has no supported plot and was skipped")),
                    }
                }
            } else if uri.contains("chartex") || uri.contains("chartEx") {
                cx.warn("a histogram, waterfall, treemap… chart without its chart part was skipped");
            } else {
                cx.warn("an embedded object (SmartArt, OLE…) was skipped");
            }
            Ok(None)
        }
        "sp" | "cxnSp" => {
            let Some(s) = read_shape(obj, anchor, depth > 0) else { return Ok(None) };
            let id = cx.next_object_id();
            sheet.shapes.push(Shape { id, ..s });
            Ok(Some(DrawingRef::Shape(id)))
        }
        "grpSp" => {
            if depth >= MAX_GROUP_DEPTH {
                cx.warn("deeply nested grouped shapes were skipped");
                return Ok(None);
            }
            let id = cx.next_object_id();
            let xf = obj_xfrm(obj);
            let map = GroupMap::new(xf, place);
            let mut members = Vec::new();
            let mut member_rotations = Vec::new();
            for child in drawing_objects(&obj.children) {
                let cxf = obj_xfrm(child);
                // A nested group's own rotation is kept on the group; its box isn't swapped.
                let rot = if child.name == "grpSp" { 0 } else { xfrm_rot(cxf) };
                let rect = map.child(cxf, quarter_turn(rot));
                let place = Place { anchor: sheet.rect_anchor(rect, anchor.mode), rect, scale: map.scale, flip: map.flip };
                let Some(m) = read_object(cx, rels, sheet, child, place, depth + 1)? else { continue };
                if rot != 0 {
                    member_rotations.push((m, rot));
                }
                members.push(m);
            }
            if members.is_empty() {
                return Ok(None);
            }
            let name = obj.path(&["nvGrpSpPr", "cNvPr"]).and_then(|c| c.attr("name")).unwrap_or("").to_string();
            let (child_offset, child_extent) = map.child_box();
            let flip = xfrm_flip(xf);
            let rotation = xfrm_rot(xf);
            if rotates(rotation) {
                cx.warn("a rotated group was laid out without its rotation");
            }
            if member_rotations.iter().any(|&(_, r)| rotates(r)) {
                cx.warn("a rotated picture or shape in a group is drawn without its rotation");
            }
            if flip.0 || flip.1 {
                cx.warn("a flipped group was laid out mirrored, but its pictures and shapes aren't drawn mirrored");
            }
            sheet.groups.push(DrawingGroup { id, name, anchor, child_offset, child_extent, flip, rotation, member_rotations, members });
            Ok(Some(DrawingRef::Group(id)))
        }
        _ => Ok(None),
    }
}

/// A shape at `anchor`; `grouped` when it sits in a group, at its group's child-space transform.
fn read_shape(obj: &El, anchor: Anchor, grouped: bool) -> Option<Shape> {
    let sp = obj.child("spPr")?;
    if let Some(shape) = ink::read(obj, anchor, grouped) {
        return Some(shape);
    }
    let prst = sp.child("prstGeom").and_then(|g| g.attr("prst")).unwrap_or("rect");
    let tx_box = obj.path(&["nvSpPr", "cNvSpPr"]).is_some_and(|c| c.flag("txBox", false));
    let ln = sp.child("ln");
    let arrow = ln.is_some_and(|l| l.child("tailEnd").or_else(|| l.child("headEnd")).is_some_and(|t| t.attr("type").is_some_and(|v| v != "none")));
    let kind = match prst {
        _ if tx_box => ShapeKind::TextBox,
        "roundRect" => ShapeKind::RoundedRectangle,
        "ellipse" => ShapeKind::Ellipse,
        "triangle" => ShapeKind::Triangle,
        "line" | "straightConnector1" if arrow => ShapeKind::Arrow,
        "line" | "straightConnector1" => ShapeKind::Line,
        "rightArrow" => ShapeKind::Arrow,
        _ => ShapeKind::Rectangle,
    };
    let fill = sp.child("solidFill").and_then(dml_color).unwrap_or_default();
    let line = ln.and_then(|l| l.child("solidFill")).and_then(dml_color).unwrap_or_default();
    let text = obj
        .child("txBody")
        .map(|tb| {
            tb.kids("p").map(|p| p.kids("r").filter_map(|r| r.child("t")).map(|t| t.text.as_str()).collect::<String>()).collect::<Vec<_>>().join("\n")
        })
        .unwrap_or_default();
    let mut format = read_text_format(obj.child("txBody"));
    format.no_line = ln.is_some_and(|l| l.child("noFill").is_some());
    Some(Shape { id: 0, kind, anchor, fill, line, text, format })
}

/// A shape's text formatting: vertical anchor (`a:bodyPr anchor`), the first paragraph's
/// alignment (`a:pPr algn`) and the first run's properties (`a:rPr`: size, bold, italic, fill,
/// outline, `a:latin` typeface).
fn read_text_format(body: Option<&El>) -> ShapeFormat {
    let mut f = ShapeFormat::default();
    let Some(body) = body else { return f };
    f.anchor = body.child("bodyPr").and_then(|b| b.attr("anchor")).map(|a| match a {
        "ctr" => ShapeTextAnchor::Middle,
        "b" => ShapeTextAnchor::Bottom,
        _ => ShapeTextAnchor::Top,
    });
    let first_p = body.kids("p").next();
    f.align = first_p.and_then(|p| p.child("pPr")).and_then(|p| p.attr("algn")).map(|a| match a {
        "ctr" => ShapeTextAlign::Center,
        "r" => ShapeTextAlign::Right,
        _ => ShapeTextAlign::Left,
    });
    let rpr = body.kids("p").flat_map(|p| p.kids("r")).find_map(|r| r.child("rPr"));
    if let Some(r) = rpr {
        // `sz` is in hundredths of a point (100..=400000 in the schema).
        f.font_size = r.attr_f64("sz").filter(|v| v.is_finite()).map(|v| (v / 100.0).clamp(1.0, 4000.0) as f32);
        f.bold = r.flag("b", false);
        f.italic = r.flag("i", false);
        f.font = r.child("latin").and_then(|l| l.attr("typeface")).filter(|t| !t.is_empty() && !t.starts_with('+')).map(str::to_string);
        f.text_color = r.child("solidFill").and_then(dml_color);
        f.text_outline = r.child("ln").and_then(|l| {
            let c = l.child("solidFill").and_then(dml_color)?;
            // Width in EMU (12700 per point); Excel's default hairline is 0.75 pt.
            let w = l.attr_f64("w").filter(|w| w.is_finite()).map_or(0.75, |w| (w / 12700.0).clamp(0.0, 100.0)) as f32;
            Some((c, w))
        });
    }
    f
}

// ---------------------------------------------------------------- writing

/// The opening `<xdr:twoCellAnchor>` for a model anchor, carrying Excel's `editAs` when the mode
/// is not the default: omitted for "Move and size with cells", `editAs="oneCell"` for "Move but
/// don't size", `editAs="absolute"` for "Don't move or size". (Excel writes pictures this way;
/// the standalone `oneCellAnchor`/`absoluteAnchor` elements are read on import but not written.)
/// Returns the opening tag and the matching closing tag.
fn anchor_open(sheet: &Sheet, a: &Anchor) -> (String, &'static str) {
    let px = |v: f64| ((v.max(0.0)) * EMU_PER_PX).round() as i64;
    let x1 = sheet.col_left(a.cell.col) + a.dx as f64;
    let y1 = sheet.row_top(a.cell.row) + a.dy as f64;
    let x2 = x1 + a.width.max(0.0) as f64;
    let y2 = y1 + a.height.max(0.0) as f64;
    let tc = sheet.col_at(x2);
    let tr = sheet.row_at(y2);
    let tdx = x2 - sheet.col_left(tc);
    let tdy = y2 - sheet.row_top(tr);
    let edit = match a.mode {
        AnchorMode::MoveAndSize => "",
        AnchorMode::MoveOnly => " editAs=\"oneCell\"",
        AnchorMode::Absolute => " editAs=\"absolute\"",
    };
    let open = format!(
        "<xdr:twoCellAnchor{edit}><xdr:from><xdr:col>{}</xdr:col><xdr:colOff>{}</xdr:colOff><xdr:row>{}</xdr:row><xdr:rowOff>{}</xdr:rowOff></xdr:from><xdr:to><xdr:col>{tc}</xdr:col><xdr:colOff>{}</xdr:colOff><xdr:row>{tr}</xdr:row><xdr:rowOff>{}</xdr:rowOff></xdr:to>",
        a.cell.col,
        px(a.dx as f64),
        a.cell.row,
        px(a.dy as f64),
        px(tdx),
        px(tdy)
    );
    (open, "</xdr:twoCellAnchor>")
}

/// EMUs for a length or position in pixels.
fn emu(v: f64) -> i64 {
    if v.is_finite() { (v * EMU_PER_PX).round().clamp(-1e15, 1e15) as i64 } else { 0 }
}

/// An `a:xfrm`/`xdr:xfrm` with an offset and extent in EMUs and a rotation (omitted when 0).
fn xfrm(tag: &str, off: (i64, i64), ext: (i64, i64), rot: i64) -> String {
    let rot = if rot == 0 { String::new() } else { format!(" rot=\"{rot}\"") };
    format!("<{tag}{rot}><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></{tag}>", off.0, off.1, ext.0.max(0), ext.1.max(0))
}

pub fn srgb(c: &Color, theme: &Theme) -> Option<String> {
    let [r, g, b] = c.resolve(theme)?;
    Some(format!("<a:srgbClr val=\"{r:02X}{g:02X}{b:02X}\"/>"))
}

/// One drawing object to write.
pub enum Obj<'a> {
    Image(&'a Image, String),
    Chart(&'a gridcraft_model::Chart, String),
    Shape(&'a Shape),
}

impl Obj<'_> {
    fn anchor(&self) -> &Anchor {
        match self {
            Obj::Image(o, _) => &o.anchor,
            Obj::Chart(o, _) => &o.anchor,
            Obj::Shape(o) => &o.anchor,
        }
    }

    fn drawing_ref(&self) -> DrawingRef {
        match self {
            Obj::Image(o, _) => DrawingRef::Image(o.id),
            Obj::Chart(o, _) => DrawingRef::Chart(o.id),
            Obj::Shape(o) => DrawingRef::Shape(o.id),
        }
    }
}

/// How a group's members are written: mirrored about the group's box `b` on the flipped axes,
/// then the linear map from sheet pixels into the group's child coordinate space (EMUs).
struct ChildSpace {
    b: ObjectRect,
    flip: (bool, bool),
    origin_px: (f64, f64),
    origin_emu: (f64, f64),
    /// EMUs per pixel on each axis.
    factor: (f64, f64),
}

impl ChildSpace {
    /// The child space of group `g`, now spanning `b` and written at `off` in its parent's space,
    /// with members mirrored on the axes in `flip` (the group's and its parents' flips). The
    /// space the group was opened (or last moved) with is kept: its anchor box maps onto its
    /// child box. An axis without extent gets the group's own offset at 1:1.
    fn new(sheet: &Sheet, g: &DrawingGroup, b: ObjectRect, off: (i64, i64), flip: (bool, bool)) -> ChildSpace {
        let r0 = sheet.anchor_rect(&g.anchor);
        // Per axis: (sheet origin, child origin, EMUs per pixel).
        let axis = |start: f64, len: f64, start0: f64, len0: f64, ch_off: i64, span: i64, off: i64| {
            if len > 0.0 && len0 > 0.0 && span > 0 { (start0, ch_off as f64, span as f64 / len0) } else { (start, off as f64, EMU_PER_PX) }
        };
        let (x0, ex, fx) = axis(b.x, b.w, r0.x, r0.w, g.child_offset.0, g.child_extent.0, off.0);
        let (y0, ey, fy) = axis(b.y, b.h, r0.y, r0.h, g.child_offset.1, g.child_extent.1, off.1);
        ChildSpace { b, flip, origin_px: (x0, y0), origin_emu: (ex, ey), factor: (fx, fy) }
    }

    /// The offset and extent (EMUs) of a member with sheet box `r`. A member turned a quarter
    /// (`turned`) is placed at its turned box: its own box is that box swapped about its centre.
    fn place(&self, mut r: ObjectRect, turned: bool) -> ((i64, i64), (i64, i64)) {
        if self.flip.0 {
            r.x = 2.0 * self.b.x + self.b.w - r.x - r.w;
        }
        if self.flip.1 {
            r.y = 2.0 * self.b.y + self.b.h - r.y - r.h;
        }
        let (mut x, mut y) =
            (self.origin_emu.0 + (r.x - self.origin_px.0) * self.factor.0, self.origin_emu.1 + (r.y - self.origin_px.1) * self.factor.1);
        let (mut w, mut h) = (r.w * self.factor.0, r.h * self.factor.1);
        if turned {
            (x, y) = (x + (w - h) / 2.0, y + (h - w) / 2.0);
            (w, h) = (h, w);
        }
        let e = |v: f64| if v.is_finite() { v.round().clamp(-1e15, 1e15) as i64 } else { 0 };
        ((e(x), e(y)), (e(w), e(h)))
    }
}

/// A drawing part being written.
struct DrawingOut<'a> {
    s: String,
    sheet: &'a Sheet,
    theme: &'a Theme,
    objs: &'a [Obj<'a>],
    /// Where each object is in `objs` (the first, should ids repeat).
    index: HashMap<DrawingRef, usize>,
    /// Objects already written, by index in `objs`, and groups already written: each goes out
    /// once.
    done: Vec<bool>,
    groups_done: HashSet<u32>,
    next_id: usize,
}

impl DrawingOut<'_> {
    fn take_id(&mut self) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// A member's box; a group's is the box around its members that are written, except that a
    /// rotated group keeps its own box (it turns about the box's centre, so a box measured from
    /// fewer members, or from turned ones, would move them).
    fn rect(&self, r: DrawingRef) -> Option<ObjectRect> {
        let members =
            self.sheet.drawing_rect_with(r, &|o| self.index.get(&o).and_then(|&i| self.objs.get(i)).map(|o| self.sheet.anchor_rect(o.anchor())));
        match r {
            DrawingRef::Group(id) => match self.sheet.drawing_group(id) {
                Some(g) if g.rotated() => members.map(|_| self.sheet.anchor_rect(&g.anchor)),
                _ => members,
            },
            _ => members,
        }
    }

    /// A `grpSp` for group `id` spanning `b`, at `off`/`ext` in its parent's space, with its
    /// members back to front in its own child space. `flip` is the axes enclosing groups flip.
    fn group(&mut self, id: u32, b: ObjectRect, off: (i64, i64), ext: (i64, i64), flip: (bool, bool), depth: usize) {
        let sheet = self.sheet;
        let Some(g) = sheet.drawing_group(id) else { return };
        if depth >= MAX_GROUP_DEPTH || !self.groups_done.insert(id) {
            return;
        }
        let flip = (flip.0 != g.flip.0, flip.1 != g.flip.1);
        let space = ChildSpace::new(sheet, g, b, off, flip);
        let (ch_off, ch_ext) = space.place(b, false);
        let cid = self.take_id();
        let name = if g.name.is_empty() { format!("Group {}", cid - 1) } else { g.name.clone() };
        let mut attrs = String::new();
        if g.rotation != 0 {
            let _ = write!(attrs, " rot=\"{}\"", g.rotation);
        }
        if g.flip.0 {
            attrs.push_str(" flipH=\"1\"");
        }
        if g.flip.1 {
            attrs.push_str(" flipV=\"1\"");
        }
        let _ = write!(
            self.s,
            "<xdr:grpSp><xdr:nvGrpSpPr><xdr:cNvPr id=\"{cid}\" name=\"{}\"/><xdr:cNvGrpSpPr/></xdr:nvGrpSpPr><xdr:grpSpPr><a:xfrm{attrs}><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/><a:chOff x=\"{}\" y=\"{}\"/><a:chExt cx=\"{}\" cy=\"{}\"/></a:xfrm></xdr:grpSpPr>",
            esc_attr(&name),
            off.0,
            off.1,
            ext.0.max(0),
            ext.1.max(0),
            ch_off.0,
            ch_off.1,
            ch_ext.0.max(0),
            ch_ext.1.max(0)
        );
        for m in &g.members {
            let Some(r) = self.rect(*m) else { continue };
            let rot = g.member_rotations.iter().find(|(o, _)| o == m).map_or(0, |&(_, rot)| rot);
            let (moff, mext) = space.place(r, quarter_turn(rot));
            match m {
                DrawingRef::Group(n) => self.group(*n, r, moff, mext, flip, depth + 1),
                leaf => {
                    let Some(&i) = self.index.get(leaf) else { continue };
                    let (Some(o), Some(false)) = (self.objs.get(i), self.done.get(i).copied()) else { continue };
                    if let Some(d) = self.done.get_mut(i) {
                        *d = true;
                    }
                    let oid = self.take_id();
                    write_obj(&mut self.s, self.theme, o, oid, moff, mext, rot);
                }
            }
        }
        self.s.push_str("</xdr:grpSp>");
    }
}

/// The drawing part for a sheet's objects (relationship ids already assigned). A grouped object
/// goes out with its outermost group: one anchor holding the `grpSp`, written where the group's
/// first object comes in `objs`.
pub fn write_drawing(sheet: &Sheet, theme: &Theme, objs: &[Obj<'_>]) -> String {
    let mut index = HashMap::new();
    for (i, o) in objs.iter().enumerate() {
        index.entry(o.drawing_ref()).or_insert(i);
    }
    let mut out = DrawingOut {
        s: String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<xdr:wsDr xmlns:xdr=\"http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\">",
        ),
        sheet,
        theme,
        objs,
        index,
        done: vec![false; objs.len()],
        groups_done: HashSet::new(),
        next_id: 2,
    };
    for (i, o) in objs.iter().enumerate() {
        if let Some(gid) = sheet.outer_group(o.drawing_ref())
            && !out.groups_done.contains(&gid)
            && let (Some(g), Some(b)) = (sheet.drawing_group(gid), out.rect(DrawingRef::Group(gid)))
        {
            let (open, close) = anchor_open(sheet, &sheet.rect_anchor(b, g.anchor.mode));
            out.s.push_str(&open);
            out.group(gid, b, (emu(b.x), emu(b.y)), (emu(b.w), emu(b.h)), (false, false), 0);
            out.s.push_str("<xdr:clientData/>");
            out.s.push_str(close);
        }
        if out.done.get(i).copied().unwrap_or(true) {
            continue;
        }
        if let Some(d) = out.done.get_mut(i) {
            *d = true;
        }
        let a = o.anchor();
        let (open, close) = anchor_open(sheet, a);
        out.s.push_str(&open);
        let id = out.take_id();
        write_obj(&mut out.s, theme, o, id, (0, 0), (emu(a.width.max(0.0) as f64), emu(a.height.max(0.0) as f64)), 0);
        out.s.push_str("<xdr:clientData/>");
        out.s.push_str(close);
    }
    out.s.push_str("</xdr:wsDr>");
    out.s
}

/// One object's element, with `cNvPr` id `id` and its transform at `off`/`ext` (EMUs), rotated
/// by `rot`.
fn write_obj(s: &mut String, theme: &Theme, o: &Obj<'_>, id: usize, off: (i64, i64), ext: (i64, i64), rot: i64) {
    match o {
        Obj::Image(img, rid) => {
            let _ = write!(
                s,
                "<xdr:pic><xdr:nvPicPr><xdr:cNvPr id=\"{id}\" name=\"Picture {}\" descr=\"{}\"/><xdr:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></xdr:cNvPicPr></xdr:nvPicPr><xdr:blipFill><a:blip r:embed=\"{rid}\"/><a:stretch><a:fillRect/></a:stretch></xdr:blipFill><xdr:spPr>{}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></xdr:spPr></xdr:pic>",
                id - 1,
                esc_attr(&img.alt),
                xfrm("a:xfrm", off, ext, rot)
            );
        }
        Obj::Chart(ch, rid) if crate::chartex::is_chartex(ch.kind) => {
            // Chartex frames come with a fallback for readers that don't know them.
            let (prefix, ns) = crate::chartex::requires(ch.kind);
            let _ = write!(
                s,
                "<mc:AlternateContent xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\"><mc:Choice xmlns:{prefix}=\"{ns}\" Requires=\"{prefix}\"><xdr:graphicFrame macro=\"\"><xdr:nvGraphicFramePr><xdr:cNvPr id=\"{id}\" name=\"Chart {}\"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr>{}<a:graphic><a:graphicData uri=\"http://schemas.microsoft.com/office/drawing/2014/chartex\"><cx:chart xmlns:cx=\"http://schemas.microsoft.com/office/drawing/2014/chartex\" r:id=\"{rid}\"/></a:graphicData></a:graphic></xdr:graphicFrame></mc:Choice><mc:Fallback><xdr:sp macro=\"\" textlink=\"\"><xdr:nvSpPr><xdr:cNvPr id=\"0\" name=\"\"/><xdr:cNvSpPr><a:spLocks noTextEdit=\"1\"/></xdr:cNvSpPr></xdr:nvSpPr><xdr:spPr>{}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></xdr:spPr><xdr:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang=\"en-US\"/><a:t>This chart type can't be shown by this application.</a:t></a:r></a:p></xdr:txBody></xdr:sp></mc:Fallback></mc:AlternateContent>",
                id - 1,
                xfrm("xdr:xfrm", off, ext, rot),
                xfrm("a:xfrm", off, ext, rot)
            );
        }
        Obj::Chart(_, rid) => {
            let _ = write!(
                s,
                "<xdr:graphicFrame macro=\"\"><xdr:nvGraphicFramePr><xdr:cNvPr id=\"{id}\" name=\"Chart {}\"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr>{}<a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart r:id=\"{rid}\"/></a:graphicData></a:graphic></xdr:graphicFrame>",
                id - 1,
                xfrm("xdr:xfrm", off, ext, rot)
            );
        }
        Obj::Shape(sh) => {
            if let Some(xml) = ink::write(sh, theme, id, off, ext, rot) {
                s.push_str(&xml);
                return;
            }
            let (prst, tx) = match sh.kind {
                ShapeKind::Rectangle => ("rect", false),
                ShapeKind::RoundedRectangle => ("roundRect", false),
                ShapeKind::Ellipse => ("ellipse", false),
                ShapeKind::Triangle => ("triangle", false),
                ShapeKind::Line | ShapeKind::Arrow => ("line", false),
                ShapeKind::TextBox => ("rect", true),
                ShapeKind::Icon | ShapeKind::Ink => ("rect", false),
            };
            let _ = write!(
                s,
                "<xdr:sp macro=\"\" textlink=\"\"><xdr:nvSpPr><xdr:cNvPr id=\"{id}\" name=\"Shape {}\"/><xdr:cNvSpPr{}/></xdr:nvSpPr><xdr:spPr>{}<a:prstGeom prst=\"{prst}\"><a:avLst/></a:prstGeom>",
                id - 1,
                if tx { " txBox=\"1\"" } else { "" },
                xfrm("a:xfrm", off, ext, rot)
            );
            if let Some(c) = srgb(&sh.fill, theme) {
                let _ = write!(s, "<a:solidFill>{c}</a:solidFill>");
            }
            let line_c = srgb(&sh.line, theme);
            if sh.format.no_line {
                s.push_str("<a:ln><a:noFill/></a:ln>");
            } else if line_c.is_some() || sh.kind == ShapeKind::Arrow {
                s.push_str("<a:ln w=\"9525\">");
                match line_c {
                    Some(c) => {
                        let _ = write!(s, "<a:solidFill>{c}</a:solidFill>");
                    }
                    None => s.push_str("<a:solidFill><a:srgbClr val=\"000000\"/></a:solidFill>"),
                }
                if sh.kind == ShapeKind::Arrow {
                    s.push_str("<a:tailEnd type=\"triangle\"/>");
                }
                s.push_str("</a:ln>");
            }
            s.push_str("</xdr:spPr>");
            if !sh.text.is_empty() {
                let f = &sh.format;
                let anchor = match f.anchor {
                    Some(ShapeTextAnchor::Top) => " anchor=\"t\"",
                    Some(ShapeTextAnchor::Middle) => " anchor=\"ctr\"",
                    Some(ShapeTextAnchor::Bottom) => " anchor=\"b\"",
                    None if sh.kind == ShapeKind::TextBox => " anchor=\"t\"",
                    None => "",
                };
                let _ = write!(s, "<xdr:txBody><a:bodyPr wrap=\"square\" rtlCol=\"0\"{anchor}/><a:lstStyle/>");
                let ppr = match f.align {
                    None => "",
                    Some(ShapeTextAlign::Left) => "<a:pPr algn=\"l\"/>",
                    Some(ShapeTextAlign::Center) => "<a:pPr algn=\"ctr\"/>",
                    Some(ShapeTextAlign::Right) => "<a:pPr algn=\"r\"/>",
                };
                let (rpr_attrs, rpr_body) = run_props(f, theme);
                for line in sh.text.split('\n') {
                    if line.is_empty() {
                        let _ = write!(s, "<a:p>{ppr}<a:endParaRPr lang=\"en-US\"{rpr_attrs}/></a:p>");
                    } else if rpr_body.is_empty() {
                        let _ = write!(s, "<a:p>{ppr}<a:r><a:rPr lang=\"en-US\"{rpr_attrs}/><a:t>{}</a:t></a:r></a:p>", esc(line));
                    } else {
                        let _ = write!(s, "<a:p>{ppr}<a:r><a:rPr lang=\"en-US\"{rpr_attrs}>{rpr_body}</a:rPr><a:t>{}</a:t></a:r></a:p>", esc(line));
                    }
                }
                s.push_str("</xdr:txBody>");
            }
            s.push_str("</xdr:sp>");
        }
    }
}

/// A text run's `a:rPr` attributes (size, bold, italic) and children in schema order (outline,
/// fill, typeface).
fn run_props(f: &ShapeFormat, theme: &Theme) -> (String, String) {
    let mut attrs = String::new();
    if let Some(sz) = f.font_size.filter(|v| v.is_finite()) {
        let _ = write!(attrs, " sz=\"{}\"", (sz.clamp(1.0, 4000.0) * 100.0).round() as u32);
    }
    if f.bold {
        attrs.push_str(" b=\"1\"");
    }
    if f.italic {
        attrs.push_str(" i=\"1\"");
    }
    let mut body = String::new();
    if let Some((c, w)) = &f.text_outline
        && let Some(c) = srgb(c, theme)
    {
        let emu = (w.clamp(0.0, 100.0) * 12700.0).round() as u32;
        let _ = write!(body, "<a:ln w=\"{emu}\"><a:solidFill>{c}</a:solidFill></a:ln>");
    }
    if let Some(c) = f.text_color.as_ref().and_then(|c| srgb(c, theme)) {
        let _ = write!(body, "<a:solidFill>{c}</a:solidFill>");
    }
    if let Some(font) = &f.font {
        let _ = write!(body, "<a:latin typeface=\"{}\"/>", esc(font));
    }
    (attrs, body)
}

/// File extension for an image MIME type.
pub fn image_ext(mime: &str, data: &[u8]) -> &'static str {
    match mime.to_ascii_lowercase().as_str() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpeg",
        "image/gif" => "gif",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        "image/svg+xml" => "svg",
        "image/x-emf" => "emf",
        "image/x-wmf" => "wmf",
        "image/webp" => "webp",
        _ => {
            if data.starts_with(b"\x89PNG") {
                "png"
            } else if data.starts_with(&[0xFF, 0xD8]) {
                "jpeg"
            } else if data.starts_with(b"GIF8") {
                "gif"
            } else {
                "png"
            }
        }
    }
}

pub fn image_content_type(ext: &str) -> &'static str {
    match ext {
        "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tiff" => "image/tiff",
        "svg" => "image/svg+xml",
        "emf" => "image/x-emf",
        "wmf" => "image/x-wmf",
        "webp" => "image/webp",
        _ => "image/png",
    }
}
