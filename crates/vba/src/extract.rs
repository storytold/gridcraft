//! Pulls module source text out of a `vbaProject.bin` package, via the `ovba` crate (which
//! handles the `[MS-CFB]` compound-file container and `[MS-OVBA]` source decompression).

/// One module's decoded VBA source, as it appears in `vbaProject.bin` — a workbook usually has
/// one of these per sheet plus any standalone modules the author added.
#[derive(Debug, Clone)]
pub struct ModuleSource {
    pub name: String,
    pub source: String,
}

/// Reads every module's source code out of a raw `vbaProject.bin` package's bytes.
///
/// A module that fails to decompress (corrupt stream, unexpected encoding) is skipped rather than
/// failing the whole project — one bad module shouldn't hide the others.
pub fn extract(vba_project_bytes: &[u8]) -> Result<Vec<ModuleSource>, String> {
    let project = ovba::open_project(vba_project_bytes.to_vec()).map_err(|e| e.to_string())?;
    Ok(project
        .modules
        .iter()
        .filter_map(|m| project.module_source(&m.name).ok().map(|source| ModuleSource { name: m.name.clone(), source }))
        .collect())
}
