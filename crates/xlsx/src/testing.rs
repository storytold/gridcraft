//! Test support: builds password-encrypted workbooks in code, so tests here and in the crates
//! above (engine, UI) never need an encrypted file on disk. Compiled for this crate's tests and,
//! elsewhere, only with the `test-support` feature (a dev-dependency feature, never shipped).
//!
//! Salts and keys are fixed so the output is deterministic. This is not an encryptor for real
//! use: a fixed key protects nothing.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use aes::cipher::{BlockCipherEncrypt, KeyInit};

use crate::crypto::{
    BLOCK_KEY, BLOCK_VERIFIER_HASH, BLOCK_VERIFIER_INPUT, F_AES, F_CRYPTOAPI, Hash, PASSWORD_ENCRYPTOR, Params, SEGMENT, derive, iterated_hash,
    standard_key,
};

/// The spin count `encrypt_workbook` uses: Excel's default.
pub const SPIN_COUNT: u32 = 100_000;

const PASSWORD_SALT: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
const KEY_DATA_SALT: [u8; 16] = [16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31];
const SECRET_KEY: [u8; 32] =
    [64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95];
const VERIFIER: [u8; 16] = [32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47];

/// `package` (normally an `.xlsx` zip) as Excel stores it with a password: Agile encryption
/// (AES-256, SHA-512) inside a compound file.
pub fn encrypt_workbook(package: &[u8], password: &str) -> Vec<u8> {
    let (info, encrypted) = encrypt_agile(package, password, SPIN_COUNT);
    compound_file(&[("EncryptionInfo", &info), ("EncryptedPackage", &encrypted)])
}

/// The same with Standard encryption (AES-128, SHA-1), as Excel 2007 wrote it.
pub fn encrypt_workbook_standard(package: &[u8], password: &str) -> Vec<u8> {
    let (info, encrypted) = encrypt_standard(package, password);
    compound_file(&[("EncryptionInfo", &info), ("EncryptedPackage", &encrypted)])
}

/// A version 3 compound file (512-byte sectors) holding `streams` under the root, laid out as
/// Excel lays it out: streams under 4096 bytes in the mini stream, larger ones in regular
/// sectors. Up to 109 FAT sectors (about 7 MB of streams).
pub fn compound_file(streams: &[(&str, &[u8])]) -> Vec<u8> {
    const SECTOR: usize = 512;
    const MINI: usize = 64;
    const CUTOFF: usize = 4096;
    const FREE: u32 = 0xFFFF_FFFF;
    const END: u32 = 0xFFFF_FFFE;
    const FAT_SECTOR: u32 = 0xFFFF_FFFD;
    let is_mini = |d: &[u8]| d.len() < CUTOFF;

    // The mini stream: small streams in 64-byte mini sectors, chained by the mini FAT.
    let mut container = vec![];
    let mut mini_fat: Vec<u32> = vec![];
    let mut mini_starts = vec![];
    for (_, d) in streams {
        if !is_mini(d) || d.is_empty() {
            mini_starts.push(END);
            continue;
        }
        let first = mini_fat.len();
        let n = d.len().div_ceil(MINI);
        for k in 0..n {
            mini_fat.push(if k + 1 == n { END } else { (first + k + 1) as u32 });
        }
        mini_starts.push(first as u32);
        container.extend_from_slice(d);
        container.resize((first + n) * MINI, 0);
    }

    // Regular-sector blobs, in order: directory, mini FAT, mini stream, then each large stream.
    let dir_len = (streams.len() + 1).div_ceil(4) * SECTOR;
    let mut blobs: Vec<Vec<u8>> = vec![vec![0; dir_len], mini_fat.iter().flat_map(|v| v.to_le_bytes()).collect(), container.clone()];
    blobs.extend(streams.iter().map(|(_, d)| if is_mini(d) { vec![] } else { d.to_vec() }));
    let counts: Vec<usize> = blobs.iter().map(|b| b.len().div_ceil(SECTOR)).collect();
    let body: usize = counts.iter().sum();
    let mut fat_sectors = 1;
    while fat_sectors * 128 < fat_sectors + body {
        fat_sectors += 1;
    }
    assert!(fat_sectors <= 109, "the test writer has no DIFAT sectors");
    let mut fat = vec![FREE; fat_sectors * 128];
    fat[..fat_sectors].fill(FAT_SECTOR);
    let mut starts = vec![];
    let mut next = fat_sectors;
    for &n in &counts {
        if n == 0 {
            starts.push(END);
            continue;
        }
        starts.push(next as u32);
        for k in 0..n {
            fat[next + k] = if k + 1 == n { END } else { (next + k + 1) as u32 };
        }
        next += n;
    }

    let put16 = |b: &mut [u8], at: usize, v: u16| b[at..at + 2].copy_from_slice(&v.to_le_bytes());
    let put32 = |b: &mut [u8], at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    // Directory: the root (owning the mini stream), then the streams chained as right siblings.
    let dir = &mut blobs[0];
    for k in 0..dir_len / 128 {
        for at in [68, 72, 76] {
            put32(dir, k * 128 + at, FREE);
        }
    }
    let mut entry = |k: usize, name: &str, kind: u8, start: u32, size: usize| {
        let e = k * 128;
        let units: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        dir[e..e + units.len()].copy_from_slice(&units);
        put16(dir, e + 64, units.len() as u16 + 2);
        dir[e + 66] = kind;
        dir[e + 67] = 1; // black
        if kind == 5 && !streams.is_empty() {
            put32(dir, e + 76, 1);
        }
        if kind == 2 && k < streams.len() {
            put32(dir, e + 72, k as u32 + 1);
        }
        put32(dir, e + 116, start);
        put32(dir, e + 120, size as u32);
    };
    entry(0, "Root Entry", 5, starts[2], container.len());
    for (k, (name, data)) in streams.iter().enumerate() {
        let start = if is_mini(data) { mini_starts[k] } else { starts[3 + k] };
        entry(k + 1, name, 2, start, data.len());
    }

    let mut out = vec![0u8; SECTOR * (1 + fat_sectors + body)];
    out[..8].copy_from_slice(&crate::cfb::SIGNATURE);
    put16(&mut out, 24, 0x3E); // minor version
    put16(&mut out, 26, 3); // major version
    put16(&mut out, 28, 0xFFFE); // byte order
    put16(&mut out, 30, 9); // 512-byte sectors
    put16(&mut out, 32, 6); // 64-byte mini sectors
    put32(&mut out, 44, fat_sectors as u32);
    put32(&mut out, 48, starts[0]);
    put32(&mut out, 56, CUTOFF as u32);
    put32(&mut out, 60, starts[1]); // first mini FAT sector (END when there is none)
    put32(&mut out, 64, counts[1] as u32);
    put32(&mut out, 68, END); // no DIFAT sectors
    for i in 0..109 {
        put32(&mut out, 76 + i * 4, if i < fat_sectors { i as u32 } else { FREE });
    }
    for (i, v) in fat.iter().enumerate() {
        put32(&mut out, SECTOR + i * 4, *v);
    }
    for (blob, start) in blobs.iter().zip(&starts) {
        if !blob.is_empty() {
            let at = SECTOR * (1 + *start as usize);
            out[at..at + blob.len()].copy_from_slice(blob);
        }
    }
    out
}

fn encrypt_block(key: &[u8], block: &mut [u8; 16]) {
    let b = aes::Block::cast_from_core_mut(block);
    match key.len() {
        16 => aes::Aes128::new_from_slice(key).unwrap().encrypt_block(b),
        32 => aes::Aes256::new_from_slice(key).unwrap().encrypt_block(b),
        n => panic!("no {n}-byte AES in the test encryptor"),
    }
}

fn pad(data: &[u8]) -> Vec<u8> {
    let mut v = data.to_vec();
    v.resize(data.len().div_ceil(16) * 16, 0);
    v
}

fn encrypt_ecb(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut v = pad(data);
    v.as_chunks_mut::<16>().0.iter_mut().for_each(|b| encrypt_block(key, b));
    v
}

fn encrypt_cbc(key: &[u8], iv: &[u8], data: &[u8]) -> Vec<u8> {
    let mut v = pad(data);
    let mut prev: [u8; 16] = iv.try_into().unwrap();
    for b in v.as_chunks_mut::<16>().0 {
        b.iter_mut().zip(prev).for_each(|(x, p)| *x ^= p);
        encrypt_block(key, b);
        prev = *b;
    }
    v
}

fn base64_encode(b: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in b.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            s.push(if i <= c.len() { T[(n >> (18 - 6 * i)) as usize & 63] as char } else { '=' });
        }
    }
    s
}

fn package(size: usize, body: Vec<u8>) -> Vec<u8> {
    let mut p = (size as u64).to_le_bytes().to_vec();
    p.extend(body);
    p
}

/// The Agile XML descriptor: AES-256, SHA-512, a password encryptor salted with 0..16.
pub(crate) fn agile_info(password: &str, spin: u32) -> String {
    let key = Params { hash: Hash::Sha512, salt: PASSWORD_SALT.to_vec(), key_len: 32 };
    let h = iterated_hash(Hash::Sha512, password, &key.salt, spin);
    let iv = key.iv(None);
    let field = |block_key: &[u8], data: &[u8]| base64_encode(&encrypt_cbc(&derive(Hash::Sha512, &h, block_key, 32), &iv, data));
    let input = field(&BLOCK_VERIFIER_INPUT, &VERIFIER);
    let value = field(&BLOCK_VERIFIER_HASH, &Hash::Sha512.digest(&[&VERIFIER]));
    let secret = field(&BLOCK_KEY, &SECRET_KEY);
    let params = |salt: &[u8]| {
        format!(
            "saltSize=\"16\" blockSize=\"16\" keyBits=\"256\" hashSize=\"64\" cipherAlgorithm=\"AES\" cipherChaining=\"ChainingModeCBC\" hashAlgorithm=\"SHA512\" saltValue=\"{}\"",
            base64_encode(salt)
        )
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<encryption xmlns=\"http://schemas.microsoft.com/office/2006/encryption\" xmlns:p=\"http://schemas.microsoft.com/office/2006/keyEncryptor/password\"><keyData {}/><keyEncryptors><keyEncryptor uri=\"{PASSWORD_ENCRYPTOR}\"><p:encryptedKey spinCount=\"{spin}\" {} encryptedVerifierHashInput=\"{input}\" encryptedVerifierHashValue=\"{value}\" encryptedKeyValue=\"{secret}\"/></keyEncryptor></keyEncryptors></encryption>",
        params(&KEY_DATA_SALT),
        params(&PASSWORD_SALT),
    )
}

/// `EncryptionInfo` for an Agile descriptor: version 4.4, reserved 0x40, then the XML.
pub(crate) fn with_agile_header(xml: &str) -> Vec<u8> {
    let mut info = vec![4, 0, 4, 0, 0x40, 0, 0, 0];
    info.extend_from_slice(xml.as_bytes());
    info
}

/// `EncryptionInfo` and `EncryptedPackage` for `plain`, Agile (AES-256, SHA-512).
pub(crate) fn encrypt_agile(plain: &[u8], password: &str, spin: u32) -> (Vec<u8>, Vec<u8>) {
    let key_data = Params { hash: Hash::Sha512, salt: KEY_DATA_SALT.to_vec(), key_len: 32 };
    let mut body = vec![];
    for (i, segment) in plain.chunks(SEGMENT).enumerate() {
        body.extend(encrypt_cbc(&SECRET_KEY, &key_data.iv(Some(&(i as u32).to_le_bytes())), segment));
    }
    (with_agile_header(&agile_info(password, spin)), package(plain.len(), body))
}

/// `EncryptionInfo` and `EncryptedPackage` for `plain`, Standard (AES-128, SHA-1).
pub(crate) fn encrypt_standard(plain: &[u8], password: &str) -> (Vec<u8>, Vec<u8>) {
    let key = standard_key(password, &PASSWORD_SALT, 16);
    let mut header = vec![];
    for v in [F_CRYPTOAPI | F_AES, 0, 0x660E, 0x8004, 128, 0x18, 0, 0] {
        header.extend_from_slice(&u32::to_le_bytes(v));
    }
    header.extend("\0".encode_utf16().flat_map(u16::to_le_bytes));
    let mut info = vec![4, 0, 2, 0];
    info.extend_from_slice(&(F_CRYPTOAPI | F_AES).to_le_bytes());
    info.extend_from_slice(&(header.len() as u32).to_le_bytes());
    info.extend(header);
    info.extend_from_slice(&16u32.to_le_bytes());
    info.extend(PASSWORD_SALT);
    info.extend(encrypt_ecb(&key, &VERIFIER));
    info.extend_from_slice(&20u32.to_le_bytes());
    info.extend(encrypt_ecb(&key, &Hash::Sha1.digest(&[&VERIFIER])));
    (info, package(plain.len(), encrypt_ecb(&key, plain)))
}

// ---------------------------------------------------------------- Power Query packages

const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const NS_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_PKG_REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

fn block(out: &mut Vec<u8>, data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
}

fn zip(parts: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write;
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in parts {
        z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(data).unwrap();
    }
    z.finish().unwrap().into_inner()
}

/// The `DataMashup` binary stream ([MS-QDEFF]): version, the package parts (with
/// `Formulas/Section1.m` = `section`), permissions, the metadata document and empty permission
/// bindings.
pub fn mashup_stream(section: &str, metadata: &str) -> Vec<u8> {
    let package = zip(&[
        ("[Content_Types].xml", br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="text/xml"/><Default Extension="m" ContentType="application/x-ms-m"/></Types>"#),
        ("Config/Package.xml", br#"<?xml version="1.0"?><Package xmlns="http://schemas.microsoft.com/DataMashup"><Version>1.0.0</Version></Package>"#),
        ("Formulas/Section1.m", section.as_bytes()),
    ]);
    let mut meta = vec![];
    meta.extend_from_slice(&0u32.to_le_bytes());
    block(&mut meta, metadata.as_bytes());
    block(&mut meta, &[]);
    let mut out = vec![];
    out.extend_from_slice(&0u32.to_le_bytes());
    block(&mut out, &package);
    block(&mut out, br#"<?xml version="1.0"?><PermissionList/>"#);
    block(&mut out, &meta);
    block(&mut out, &[]);
    out
}

/// The custom XML part holding a mashup stream, as Excel writes it: UTF-16 with a byte order mark.
pub fn mashup_part(stream: &[u8]) -> Vec<u8> {
    let text = format!(
        r#"<?xml version="1.0" encoding="utf-16"?><DataMashup xmlns="http://schemas.microsoft.com/DataMashup">{}</DataMashup>"#,
        base64_encode(stream)
    );
    let mut out = vec![0xFF, 0xFE];
    out.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    out
}

/// A one-sheet workbook (sheet `Data`) as Excel saves one with a Power Query query: the query
/// `Sales` (its M is `sales_formula`) loads into the query table `Sales` at A1:B3 (`Region`,
/// `Amount`; North 5, South 7 from the last refresh) through connection 1, and the ordinary table
/// `Raw` at D1:E4 holds North 10, South 20, East 30.
pub fn power_query_workbook(sales_formula: &str) -> Vec<u8> {
    let section = format!("section Section1;\r\n\r\nshared Sales = {sales_formula};\r\n");
    let metadata = r#"<?xml version="1.0" encoding="utf-8"?><LocalPackageMetadataFile><Items><Item><ItemLocation><ItemType>Formula</ItemType><ItemPath>Section1/Sales</ItemPath></ItemLocation><StableEntries><Entry Type="FillEnabled" Value="l1" /><Entry Type="FillTarget" Value="sSales" /><Entry Type="FillCount" Value="l2" /></StableEntries></Item></Items></LocalPackageMetadataFile>"#;
    let mashup = mashup_part(&mashup_stream(&section, metadata));
    let cell = |r: &str, v: &str| match v.parse::<f64>() {
        Ok(_) => format!(r#"<c r="{r}"><v>{v}</v></c>"#),
        Err(_) => format!(r#"<c r="{r}" t="inlineStr"><is><t>{v}</t></is></c>"#),
    };
    let rows = [
        ["Region", "Amount", "", "Region", "Amount"],
        ["North", "5", "", "North", "10"],
        ["South", "7", "", "South", "20"],
        ["", "", "", "East", "30"],
    ];
    let mut data = String::new();
    for (i, row) in rows.iter().enumerate() {
        data.push_str(&format!(r#"<row r="{}">"#, i + 1));
        for (j, v) in row.iter().enumerate() {
            if !v.is_empty() {
                data.push_str(&cell(&format!("{}{}", ["A", "B", "C", "D", "E"][j], i + 1), v));
            }
        }
        data.push_str("</row>");
    }
    let sheet = format!(
        r#"<?xml version="1.0"?><worksheet xmlns="{NS_MAIN}" xmlns:r="{NS_REL}"><sheetData>{data}</sheetData><tableParts count="2"><tablePart r:id="rId1"/><tablePart r:id="rId2"/></tableParts></worksheet>"#
    );
    let sheet_rels = format!(
        r#"<?xml version="1.0"?><Relationships xmlns="{NS_PKG_REL}"><Relationship Id="rId1" Type="{NS_REL}/table" Target="../tables/table1.xml"/><Relationship Id="rId2" Type="{NS_REL}/table" Target="../tables/table2.xml"/></Relationships>"#
    );
    let sales = format!(
        r#"<?xml version="1.0"?><table xmlns="{NS_MAIN}" id="1" name="Sales" displayName="Sales" ref="A1:B3" tableType="queryTable" totalsRowShown="0"><autoFilter ref="A1:B3"/><tableColumns count="2"><tableColumn id="1" uniqueName="1" name="Region" queryTableFieldId="1"/><tableColumn id="2" uniqueName="2" name="Amount" queryTableFieldId="2"/></tableColumns><tableStyleInfo name="TableStyleMedium7" showFirstColumn="0" showLastColumn="0" showRowStripes="1" showColumnStripes="0"/></table>"#
    );
    let sales_rels = format!(
        r#"<?xml version="1.0"?><Relationships xmlns="{NS_PKG_REL}"><Relationship Id="rId1" Type="{NS_REL}/queryTable" Target="../queryTables/queryTable1.xml"/></Relationships>"#
    );
    let raw = format!(
        r#"<?xml version="1.0"?><table xmlns="{NS_MAIN}" id="2" name="Raw" displayName="Raw" ref="D1:E4" totalsRowShown="0"><autoFilter ref="D1:E4"/><tableColumns count="2"><tableColumn id="1" name="Region"/><tableColumn id="2" name="Amount"/></tableColumns><tableStyleInfo name="TableStyleMedium2" showFirstColumn="0" showLastColumn="0" showRowStripes="1" showColumnStripes="0"/></table>"#
    );
    let query_table = format!(
        r#"<?xml version="1.0"?><queryTable xmlns="{NS_MAIN}" name="ExternalData_1" connectionId="1" autoFormatId="16" applyNumberFormats="0" applyBorderFormats="0" applyFontFormats="0" applyPatternFormats="0" applyAlignmentFormats="0" applyWidthHeightFormats="0"><queryTableRefresh nextId="3"><queryTableFields count="2"><queryTableField id="1" name="Region" tableColumnId="1"/><queryTableField id="2" name="Amount" tableColumnId="2"/></queryTableFields></queryTableRefresh></queryTable>"#
    );
    let connections = format!(
        r#"<?xml version="1.0"?><connections xmlns="{NS_MAIN}"><connection id="1" keepAlive="1" name="Query - Sales" type="5" refreshedVersion="8" background="1" saveData="1"><dbPr connection="Provider=Microsoft.Mashup.OleDb.1;Data Source=$Workbook$;Location=Sales;Extended Properties=&quot;&quot;" command="SELECT * FROM [Sales]"/></connection></connections>"#
    );
    let workbook = format!(
        r#"<?xml version="1.0"?><workbook xmlns="{NS_MAIN}" xmlns:r="{NS_REL}"><sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>"#
    );
    let wb_rels = format!(
        r#"<?xml version="1.0"?><Relationships xmlns="{NS_PKG_REL}"><Relationship Id="rId1" Type="{NS_REL}/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId8" Type="{NS_REL}/customXml" Target="../customXml/item1.xml"/><Relationship Id="rId9" Type="{NS_REL}/connections" Target="connections.xml"/></Relationships>"#
    );
    let content_types = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/></Types>"#;
    let root_rels = format!(
        r#"<?xml version="1.0"?><Relationships xmlns="{NS_PKG_REL}"><Relationship Id="rId1" Type="{NS_REL}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
    );
    zip(&[
        ("[Content_Types].xml", content_types.as_bytes()),
        ("_rels/.rels", root_rels.as_bytes()),
        ("xl/workbook.xml", workbook.as_bytes()),
        ("xl/_rels/workbook.xml.rels", wb_rels.as_bytes()),
        ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
        ("xl/worksheets/_rels/sheet1.xml.rels", sheet_rels.as_bytes()),
        ("xl/tables/table1.xml", sales.as_bytes()),
        ("xl/tables/_rels/table1.xml.rels", sales_rels.as_bytes()),
        ("xl/tables/table2.xml", raw.as_bytes()),
        ("xl/queryTables/queryTable1.xml", query_table.as_bytes()),
        ("xl/connections.xml", connections.as_bytes()),
        ("customXml/item1.xml", &mashup),
    ])
}
