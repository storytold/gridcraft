#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Synthetic font fixtures for PDF and command integration tests. No external font assets.

/// A minimal TrueType file built in code: `chars[i]` maps to glyph `i + 1`, every glyph is
/// `advance` units wide on a 1000-unit em; `fs_type` goes into the OS/2 table.
pub fn tiny_ttf(chars: &[char], advance: u16, fs_type: u16) -> Vec<u8> {
    let be16 = |v: u16| v.to_be_bytes();
    let num_glyphs = chars.len() as u16 + 1;
    let mut head = vec![0u8; 54];
    head[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    head[18..20].copy_from_slice(&be16(1000));
    head[36..44].copy_from_slice(&[0xFF, 0x38, 0xFF, 0x06, 0x03, 0xE8, 0x03, 0x84]); // -200 -250 1000 900
    let mut hhea = vec![0u8; 36];
    hhea[4..6].copy_from_slice(&be16(800));
    hhea[6..8].copy_from_slice(&(-200i16).to_be_bytes());
    hhea[34..36].copy_from_slice(&be16(num_glyphs));
    let mut maxp = vec![0, 0, 0x50, 0];
    maxp.extend_from_slice(&be16(num_glyphs));
    let hmtx: Vec<u8> = (0..num_glyphs).flat_map(|_| [be16(advance), [0, 0]].concat()).collect();
    let mut segs: Vec<(u16, u16)> = chars.iter().enumerate().map(|(i, c)| (*c as u16, i as u16 + 1)).collect();
    segs.sort();
    segs.push((0xFFFF, 0));
    let seg_count = segs.len() as u16;
    let mut sub = Vec::new();
    sub.extend_from_slice(&be16(4));
    sub.extend_from_slice(&be16(16 + seg_count * 8));
    sub.extend_from_slice(&be16(0));
    sub.extend_from_slice(&be16(seg_count * 2));
    sub.extend_from_slice(&[0; 6]);
    segs.iter().for_each(|(c, _)| sub.extend_from_slice(&be16(*c)));
    sub.extend_from_slice(&be16(0));
    segs.iter().for_each(|(c, _)| sub.extend_from_slice(&be16(*c)));
    segs.iter().for_each(|(c, g)| sub.extend_from_slice(&be16(if *c == 0xFFFF { 1 } else { g.wrapping_sub(*c) })));
    segs.iter().for_each(|_| sub.extend_from_slice(&be16(0)));
    let mut cmap = vec![0, 0, 0, 1, 0, 3, 0, 1, 0, 0, 0, 12];
    cmap.extend_from_slice(&sub);
    if chars.iter().any(|c| *c as u32 > 0xFFFF) {
        let mut groups: Vec<_> = chars.iter().enumerate().map(|(i, c)| (*c as u32, i as u32 + 1)).collect();
        groups.sort();
        let mut sub = Vec::new();
        sub.extend_from_slice(&[0, 12, 0, 0]);
        sub.extend_from_slice(&(16u32 + groups.len() as u32 * 12).to_be_bytes());
        sub.extend_from_slice(&[0; 4]);
        sub.extend_from_slice(&(groups.len() as u32).to_be_bytes());
        for (c, g) in groups {
            for value in [c, c, g] {
                sub.extend_from_slice(&value.to_be_bytes());
            }
        }
        cmap = vec![0, 0, 0, 1, 0, 3, 0, 10, 0, 0, 0, 12];
        cmap.extend_from_slice(&sub);
    }
    let loca = vec![0u8; usize::from(num_glyphs + 1) * 2];
    let mut os2 = vec![0u8; 96];
    os2[0..2].copy_from_slice(&be16(4));
    os2[8..10].copy_from_slice(&be16(fs_type));
    os2[88..90].copy_from_slice(&be16(700));
    let tables: [(&[u8; 4], Vec<u8>); 8] =
        [(b"OS/2", os2), (b"cmap", cmap), (b"glyf", Vec::new()), (b"head", head), (b"hhea", hhea), (b"hmtx", hmtx), (b"loca", loca), (b"maxp", maxp)];
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&be16(tables.len() as u16));
    out.extend_from_slice(&[0; 6]);
    let mut offset = 12 + 16 * tables.len();
    let mut body = Vec::new();
    for (tag, data) in &tables {
        out.extend_from_slice(*tag);
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        body.extend_from_slice(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        offset = 12 + 16 * tables.len() + body.len();
    }
    out.extend_from_slice(&body);
    out
}
