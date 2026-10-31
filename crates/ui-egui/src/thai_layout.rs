//! Thai text line wrapping for committed worksheet cells.
//!
//! Clean-room implementation providing word boundary-aware line wrapping for Thai script in
//! spreadsheet cells. Uses ICU4X LSTM-based LineSegmenter for complex script boundary detection
//! and egui's resolved font metrics for greedy line placement.
//!
//! Follows the strict repository safety policy: `#![deny(clippy::unwrap_used)]`, no panics,
//! checked arithmetic, finite geometry validation, bounded budgets, and memory-capped LRU cache.

use std::borrow::Borrow;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use egui::{Color32, FontId, Painter, Rect, Vec2, pos2, vec2};
use gridcraft_engine::core::Value;
use gridcraft_engine::model::{HAlign, Style, VAlign};
use icu_segmenter::options::LineBreakOptions;
use icu_segmenter::{LineSegmenter, LineSegmenterBorrowed};

/// Maximum number of Unicode scalars permitted in a Thai text cell before falling back.
pub const MAX_THAI_SCALARS: usize = 32_767;

/// Maximum candidate measurement operations allowed per individual cell.
pub const MAX_CELL_MEASUREMENTS: usize = 512;

/// Maximum cumulative candidate bytes measured per individual cell (1 MiB).
pub const MAX_CELL_MEASURED_BYTES: usize = 1024 * 1024;

/// Maximum number of soft- or hard-wrapped output lines allowed for one cell.
pub const MAX_CELL_LAYOUT_LINES: usize = 512;

/// Maximum cache-miss candidate measurements allowed across one egui UI pass.
pub const MAX_FRAME_MEASUREMENTS: usize = 4_096;

/// Maximum cumulative cache-miss bytes measured across one egui UI pass (4 MiB).
pub const MAX_FRAME_MEASURED_BYTES: usize = 4 * 1024 * 1024;

/// Maximum entries in the Thai layout LRU cache.
pub const MAX_CACHE_ENTRIES: usize = 1_024;

/// Maximum payload memory consumed by the Thai layout LRU cache (8 MiB).
pub const MAX_CACHE_BYTES: usize = 8 * 1024 * 1024;

const MAX_NO_PROGRESS_PASSES: usize = 2;
const MAX_RETRY_PASSES_BY_MEASUREMENTS: usize = (MAX_CACHE_ENTRIES * MAX_CELL_MEASUREMENTS).div_ceil(MAX_FRAME_MEASUREMENTS);
const MAX_RETRY_PASSES_BY_BYTES: usize = (MAX_CACHE_ENTRIES * MAX_CELL_MEASURED_BYTES).div_ceil(MAX_FRAME_MEASURED_BYTES);
const MAX_RETRY_PASSES: usize =
    if MAX_RETRY_PASSES_BY_MEASUREMENTS > MAX_RETRY_PASSES_BY_BYTES { MAX_RETRY_PASSES_BY_MEASUREMENTS + 1 } else { MAX_RETRY_PASSES_BY_BYTES + 1 };

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrameBudgetLimits {
    max_measurements: usize,
    max_measured_bytes: usize,
}

impl Default for FrameBudgetLimits {
    fn default() -> Self {
        Self { max_measurements: MAX_FRAME_MEASUREMENTS, max_measured_bytes: MAX_FRAME_MEASURED_BYTES }
    }
}

/// Per-egui-pass counters for visible Thai cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct FrameBudget {
    segmentations: usize,
    measurements: usize,
    measured_bytes: usize,
}

#[derive(Clone, Copy, Debug, Default)]
struct RetryProgress {
    signature: Option<(u64, usize)>,
    passes: usize,
    no_progress_passes: usize,
    previous_cache_hits: usize,
    stopped: bool,
}

impl FrameBudget {
    fn reserve_measurement(&mut self, bytes: usize, limits: FrameBudgetLimits) -> bool {
        let Some(measurements) = self.measurements.checked_add(1) else {
            return false;
        };
        let Some(measured_bytes) = self.measured_bytes.checked_add(bytes) else {
            return false;
        };
        if measurements > limits.max_measurements || measured_bytes > limits.max_measured_bytes {
            return false;
        }
        self.measurements = measurements;
        self.measured_bytes = measured_bytes;
        true
    }
}

#[derive(Clone)]
struct FrameBudgetState {
    pass_nr: u64,
    limits: FrameBudgetLimits,
    usage: FrameBudget,
    workload_hash: u64,
    workload_items: usize,
    cache_hits: usize,
    exhausted: bool,
    suspend_misses: bool,
    retry_progress: RetryProgress,
    finish_baseline: Option<RetryProgress>,
    repaint_retry: bool,
}

impl FrameBudgetState {
    fn new(pass_nr: u64) -> Self {
        Self {
            pass_nr,
            limits: FrameBudgetLimits::default(),
            usage: FrameBudget::default(),
            workload_hash: 0xcbf2_9ce4_8422_2325,
            workload_items: 0,
            cache_hits: 0,
            exhausted: false,
            suspend_misses: false,
            retry_progress: RetryProgress::default(),
            finish_baseline: None,
            repaint_retry: false,
        }
    }

    fn start_pass(&mut self, pass_nr: u64) {
        self.pass_nr = pass_nr;
        self.usage = FrameBudget::default();
        self.workload_hash = 0xcbf2_9ce4_8422_2325;
        self.workload_items = 0;
        self.cache_hits = 0;
        self.exhausted = false;
        self.suspend_misses = self.retry_progress.stopped;
        self.finish_baseline = None;
        self.repaint_retry = false;
    }

    fn record_request(&mut self, key_hash: u64, cache_hit: bool) {
        self.workload_hash = self.workload_hash.rotate_left(7) ^ key_hash.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        self.workload_items = self.workload_items.saturating_add(1);
        if cache_hit {
            self.cache_hits = self.cache_hits.saturating_add(1);
        }
    }

    fn finish_pass(&mut self) -> bool {
        // Multiple grid paints may finish their local scope before another scope adds requests.
        // Recompute each such decision from the state before this actual egui pass so the full
        // request sequence counts once, regardless of how many scopes call this method.
        let baseline = *self.finish_baseline.get_or_insert(self.retry_progress);
        let signature = (self.workload_hash, self.workload_items);
        if !self.exhausted {
            self.retry_progress = RetryProgress::default();
            self.repaint_retry = false;
            return false;
        }

        // Keep the stopped decision even if the stable, no-eviction pass has more hits.
        // A different complete request sequence restarts evaluation on the next pass.
        if baseline.stopped && baseline.signature == Some(signature) {
            self.retry_progress = baseline;
            self.repaint_retry = false;
            return false;
        }
        let mut progress = baseline;
        if progress.signature == Some(signature) {
            if self.cache_hits > progress.previous_cache_hits {
                progress.no_progress_passes = 0;
            } else {
                progress.no_progress_passes = progress.no_progress_passes.saturating_add(1);
            }
            progress.passes = progress.passes.saturating_add(1);
        } else {
            progress.signature = Some(signature);
            progress.passes = 1;
            progress.no_progress_passes = 0;
        }
        progress.previous_cache_hits = self.cache_hits;
        progress.stopped = progress.no_progress_passes >= MAX_NO_PROGRESS_PASSES || progress.passes >= MAX_RETRY_PASSES;
        self.retry_progress = progress;
        self.repaint_retry = !progress.stopped;
        self.repaint_retry
    }
}

static NEXT_CONTEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug)]
struct ContextCacheIdentity {
    context_id: u64,
    font_generation: u64,
}

fn context_cache_identity_id() -> egui::Id {
    egui::Id::unique("gridcraft.thai_layout.context_cache_identity")
}

fn new_context_cache_identity() -> ContextCacheIdentity {
    ContextCacheIdentity { context_id: NEXT_CONTEXT_ID.fetch_add(1, Ordering::Relaxed).max(1), font_generation: 1 }
}

/// Records a new font generation for this egui context and drops plans from older font sets.
pub fn bump_font_generation(ctx: &egui::Context) {
    let id = context_cache_identity_id();
    let identity = ctx.data_mut(|data| {
        let mut identity = if let Some(identity) = data.get_temp::<ContextCacheIdentity>(id) { identity } else { new_context_cache_identity() };
        identity.font_generation = identity.font_generation.saturating_add(1).max(1);
        data.insert_temp(id, identity);
        identity
    });
    if let Ok(mut cache_guard) = CACHE.lock()
        && let Some(cache) = cache_guard.as_mut()
    {
        cache.clear_context(identity.context_id);
    }
}

fn frame_budget_key(ctx: &egui::Context) -> (egui::Id, u64) {
    let viewport_id = ctx.viewport_id();
    (egui::Id::unique(("gridcraft.thai_layout.frame_budget", viewport_id)), ctx.cumulative_pass_nr_for(viewport_id))
}

#[derive(Clone, Copy)]
struct FrameBudgetFinalizerInstalled;

/// Register one Context end-pass hook that evaluates the complete set of grid paint scopes.
pub(crate) fn install_frame_budget_finalizer(ctx: &egui::Context) {
    let id = egui::Id::unique("gridcraft.thai_layout.frame_budget_finalizer_installed");
    let should_install = ctx.data_mut(|data| {
        if data.get_temp::<FrameBudgetFinalizerInstalled>(id).is_some() {
            false
        } else {
            data.insert_temp(id, FrameBudgetFinalizerInstalled);
            true
        }
    });
    if should_install {
        ctx.on_end_pass(
            "gridcraft_thai_layout_frame_budget",
            Arc::new(|ui: &mut egui::Ui| {
                let ctx = ui.ctx().clone();
                if finish_frame_budget(&ctx) {
                    ctx.request_repaint();
                }
            }),
        );
    }
}

fn with_frame_budget<R>(ctx: &egui::Context, update: impl FnOnce(&mut FrameBudgetState) -> R) -> R {
    let (id, pass_nr) = frame_budget_key(ctx);
    ctx.data_mut(|data| {
        let state = data.get_temp_mut_or_insert_with(id, || FrameBudgetState::new(pass_nr));
        if state.pass_nr != pass_nr {
            state.start_pass(pass_nr);
        }
        update(state)
    })
}

/// Ensure the current Context/viewport/pass budget exists. Repeated calls within one egui pass are idempotent.
pub fn begin_frame_budget(ctx: &egui::Context) {
    install_frame_budget_finalizer(ctx);
    with_frame_budget(ctx, |_| ());
}

/// Returns the work charged to the current or most recently completed pass for Thai wrapping.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameBudgetUsage {
    pub segmentations: usize,
    pub measurements: usize,
    pub measured_bytes: usize,
}

/// Read counters from the active or most recently completed egui pass for this viewport.
pub fn frame_budget_usage(ctx: &egui::Context) -> FrameBudgetUsage {
    let (id, _) = frame_budget_key(ctx);
    ctx.data(|data| {
        data.get_temp::<FrameBudgetState>(id).map_or_else(FrameBudgetUsage::default, |state| FrameBudgetUsage {
            segmentations: state.usage.segmentations,
            measurements: state.usage.measurements,
            measured_bytes: state.usage.measured_bytes,
        })
    })
}

/// Approximate resident memory retained by the bounded Thai line-plan cache.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ThaiCacheUsage {
    pub entries: usize,
    pub payload_bytes: usize,
}

/// Read the current Thai line-plan cache usage, including its reserved hash table, key strings,
/// line vectors, and conservative per-allocation bookkeeping estimates.
pub fn cache_usage() -> ThaiCacheUsage {
    CACHE.lock().map_or_else(
        |_| ThaiCacheUsage::default(),
        |guard| {
            guard
                .as_ref()
                .map_or_else(ThaiCacheUsage::default, |cache| ThaiCacheUsage { entries: cache.entries.len(), payload_bytes: cache.total_bytes })
        },
    )
}

fn record_segmentation(ctx: &egui::Context) {
    with_frame_budget(ctx, |state| state.usage.segmentations = state.usage.segmentations.saturating_add(1));
}

fn reserve_frame_measurement(ctx: &egui::Context, bytes: usize) -> bool {
    with_frame_budget(ctx, |state| state.usage.reserve_measurement(bytes, state.limits))
}

fn mark_frame_budget_exhausted(ctx: &egui::Context) {
    with_frame_budget(ctx, |state| state.exhausted = true);
}

#[cfg(test)]
fn record_layout_request(ctx: &egui::Context, key: &CacheKey, cache_hit: bool) {
    with_frame_budget(ctx, |state| state.record_request(key.view().request_hash(), cache_hit));
}

/// Finalize the complete Context/viewport pass after all registered egui paint scopes have run.
fn finish_frame_budget(ctx: &egui::Context) -> bool {
    let (id, pass_nr) = frame_budget_key(ctx);
    let has_current_pass_state = ctx.data(|data| data.get_temp::<FrameBudgetState>(id).is_some_and(|state| state.pass_nr == pass_nr));
    if !has_current_pass_state {
        return false;
    }
    with_frame_budget(ctx, FrameBudgetState::finish_pass)
}

#[cfg(test)]
fn set_frame_budget_limits(ctx: &egui::Context, limits: FrameBudgetLimits) {
    with_frame_budget(ctx, |state| state.limits = limits);
}

/// Thai script segmenter using ICU4X LSTM model with compiled complex scripts data.
static THAI_SEGMENTER: OnceLock<LineSegmenterBorrowed<'static>> = OnceLock::new();

fn segmenter() -> LineSegmenterBorrowed<'static> {
    *THAI_SEGMENTER.get_or_init(|| LineSegmenter::new_lstm(LineBreakOptions::default()))
}

/// Returns true if `text` contains at least one character in the Thai Unicode block (U+0E00..=U+0E7F).
#[inline]
pub fn has_thai_chars(text: &str) -> bool {
    text.chars().any(|c| ('\u{0E00}'..='\u{0E7F}').contains(&c))
}

/// The type of break ending a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreakKind {
    /// Soft wrap chosen by layout calculation to fit within available width.
    SoftWrap,
    /// Explicit mandatory line break (`\n` or `\r\n`).
    HardBreak,
}

/// Exact UTF-8 byte ranges for a layout line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextLineRange {
    /// Byte range in original string consumed by this line, including trailing breaks/spaces.
    /// Concatenating `source_range` across all lines reconstructs the original text byte-for-byte.
    pub source_range: Range<usize>,
    /// Byte range in original string that is actually measured and painted.
    pub paint_range: Range<usize>,
    /// Break kind at the end of this line, if any.
    pub break_kind: Option<BreakKind>,
}

/// Represents one laid out line of Thai text.
#[derive(Clone, Debug)]
pub struct ThaiLayoutLine {
    pub range: TextLineRange,
    pub galley: Arc<egui::Galley>,
}

/// Completed Thai cell layout ready for painting.
#[derive(Clone, Debug)]
pub struct ThaiCellLayout {
    pub lines: Vec<ThaiLayoutLine>,
    pub size: Vec2,
}

/// Reason for falling back to legacy layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FallbackReason {
    NoThai,
    WrapDisabled,
    Rotated(i16),
    UnsupportedHAlign,
    UnsupportedVAlign,
    NumericOrDate,
    TextTooLong(usize),
    BudgetExhausted,
    InvalidWidth,
    FrameBudgetExhausted,
}

/// Result of evaluating Thai layout for a cell.
#[derive(Clone, Debug)]
pub enum ThaiLayoutResult {
    Ready(ThaiCellLayout),
    LegacyFallback(FallbackReason),
}

/// Returns whether a cell's styling and value meet all preconditions for Thai line wrapping.
pub fn is_eligible(text: &str, style: &Style, value: &Value, numeric: bool) -> Result<(), FallbackReason> {
    let scalar_count = text.chars().take(MAX_THAI_SCALARS.saturating_add(1)).count();
    if scalar_count > MAX_THAI_SCALARS {
        return Err(FallbackReason::TextTooLong(scalar_count));
    }
    if !has_thai_chars(text) {
        return Err(FallbackReason::NoThai);
    }
    if !style.align.wrap {
        return Err(FallbackReason::WrapDisabled);
    }
    if style.align.rotation != 0 {
        return Err(FallbackReason::Rotated(style.align.rotation));
    }
    if numeric || matches!(value, Value::Number(_)) {
        return Err(FallbackReason::NumericOrDate);
    }
    match style.align.h {
        HAlign::General | HAlign::Left | HAlign::Center | HAlign::Right => {}
        _ => return Err(FallbackReason::UnsupportedHAlign),
    }
    match style.align.v {
        VAlign::Top | VAlign::Center | VAlign::Bottom => {}
        _ => return Err(FallbackReason::UnsupportedVAlign),
    }
    Ok(())
}

/// Breaks `text` into paragraph segments delimited by hard line breaks (`\n` or `\r\n`).
/// Each paragraph returns its byte range in `text` excluding the hard break, and the full range including the break.
fn split_hard_breaks(text: &str) -> Vec<(Range<usize>, Range<usize>, bool)> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut i = 0;
    let len = bytes.len();

    while i < len {
        if bytes.get(i) == Some(&b'\n') {
            let content_end = if i > start && bytes.get(i - 1) == Some(&b'\r') { i - 1 } else { i };
            let full_end = i.saturating_add(1);
            out.push((start..content_end, start..full_end, true));
            start = full_end;
        }
        i = i.saturating_add(1);
    }

    if start < len {
        out.push((start..len, start..len, false));
    } else if len > 0 && bytes.get(len.saturating_sub(1)) == Some(&b'\n') {
        // Trailing newline produces an empty trailing line
        out.push((len..len, len..len, false));
    } else if len == 0 {
        out.push((0..0, 0..0, false));
    }

    out
}

/// Computes the break opportunity byte offsets within `para_text` using ICU4X line segmenter.
pub fn get_break_opportunities(para_text: &str) -> Vec<usize> {
    get_break_opportunities_with(para_text, || {})
}

fn get_break_opportunities_with(para_text: &str, mut record: impl FnMut()) -> Vec<usize> {
    if para_text.is_empty() {
        return vec![0];
    }
    record();
    let mut breaks = Vec::new();
    let seg = segmenter();
    for offset in seg.segment_str(para_text) {
        if offset <= para_text.len() {
            breaks.push(offset);
        }
    }
    if breaks.first().copied() != Some(0) {
        breaks.insert(0, 0);
    }
    if breaks.last().copied() != Some(para_text.len()) {
        breaks.push(para_text.len());
    }
    breaks.dedup();
    breaks
}

enum CacheRequest {
    Cached(CachedPlan),
    Measure(Arc<CacheKey>),
    Deferred,
}

/// Cached representation of laid out line boundaries.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CachedPlanLine {
    range: TextLineRange,
}

#[derive(Clone, Debug)]
struct CachedPlan {
    lines: Arc<[CachedPlanLine]>,
    key_hash: u64,
    fallback_reason: Option<FallbackReason>,
    accounted_bytes: usize,
    last_used: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CacheKey {
    text: String,
    family_name: String,
    context_id: u64,
    font_size_bits: u32,
    avail_width_bits: u32,
    pixels_per_point_bits: u32,
    font_gen: u64,
    zoom_pct: u16,
    halign: u8,
}

// Owned and borrowed keys use exactly the same fields and hashing order. The local
// trait permits HashMap's borrowed lookup through Arc without copying text or family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct CacheKeyRef<'a> {
    text: &'a str,
    family_name: &'a str,
    context_id: u64,
    font_size_bits: u32,
    avail_width_bits: u32,
    pixels_per_point_bits: u32,
    font_gen: u64,
    zoom_pct: u16,
    halign: u8,
}

trait CacheKeyData {
    fn view(&self) -> CacheKeyRef<'_>;
}

impl CacheKeyData for CacheKey {
    fn view(&self) -> CacheKeyRef<'_> {
        CacheKeyRef {
            text: &self.text,
            family_name: &self.family_name,
            context_id: self.context_id,
            font_size_bits: self.font_size_bits,
            avail_width_bits: self.avail_width_bits,
            pixels_per_point_bits: self.pixels_per_point_bits,
            font_gen: self.font_gen,
            zoom_pct: self.zoom_pct,
            halign: self.halign,
        }
    }
}

impl CacheKeyRef<'_> {
    fn into_owned(self) -> CacheKey {
        CacheKey {
            text: self.text.to_owned(),
            family_name: self.family_name.to_owned(),
            context_id: self.context_id,
            font_size_bits: self.font_size_bits,
            avail_width_bits: self.avail_width_bits,
            pixels_per_point_bits: self.pixels_per_point_bits,
            font_gen: self.font_gen,
            zoom_pct: self.zoom_pct,
            halign: self.halign,
        }
    }

    fn request_hash(self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }
}

impl CacheKeyData for CacheKeyRef<'_> {
    fn view(&self) -> CacheKeyRef<'_> {
        *self
    }
}

impl Hash for dyn CacheKeyData + '_ {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.view().hash(state);
    }
}

impl PartialEq for dyn CacheKeyData + '_ {
    fn eq(&self, other: &Self) -> bool {
        self.view() == other.view()
    }
}

impl Eq for dyn CacheKeyData + '_ {}

impl<'a> Borrow<dyn CacheKeyData + 'a> for Arc<CacheKey> {
    fn borrow(&self) -> &(dyn CacheKeyData + 'a) {
        self.as_ref()
    }
}

struct LayoutCache {
    entries: HashMap<Arc<CacheKey>, CachedPlan>,
    total_bytes: usize,
    base_bytes: usize,
    table_capacity_high_water: usize,
    clock: u64,
    max_entries: usize,
    max_bytes: usize,
}

const CACHE_ALLOCATOR_OVERHEAD_BYTES: usize = 32;

fn estimated_hash_table_bytes(capacity: usize) -> usize {
    if capacity == 0 {
        return 0;
    }
    // HashMap::capacity is the number of entries before growth; account for the larger bucket
    // allocation implied by its load factor, one control byte per bucket, and allocator overhead.
    let buckets = capacity.saturating_mul(8).saturating_add(6) / 7;
    let bucket_bytes = std::mem::size_of::<(Arc<CacheKey>, CachedPlan)>().saturating_add(1);
    buckets.saturating_mul(bucket_bytes).saturating_add(CACHE_ALLOCATOR_OVERHEAD_BYTES)
}

fn estimated_string_allocation_bytes(capacity: usize) -> usize {
    if capacity == 0 { 0 } else { capacity.saturating_add(CACHE_ALLOCATOR_OVERHEAD_BYTES) }
}

fn estimated_key_allocation_bytes(key: &CacheKey) -> usize {
    std::mem::size_of::<CacheKey>()
        .saturating_add(2 * std::mem::size_of::<usize>())
        .saturating_add(CACHE_ALLOCATOR_OVERHEAD_BYTES)
        .saturating_add(estimated_string_allocation_bytes(key.text.capacity()))
        .saturating_add(estimated_string_allocation_bytes(key.family_name.capacity()))
}

fn estimated_plan_allocation_bytes(plan: &CachedPlan) -> usize {
    plan.lines
        .len()
        .saturating_mul(std::mem::size_of::<CachedPlanLine>())
        .saturating_add(2 * std::mem::size_of::<usize>())
        .saturating_add(CACHE_ALLOCATOR_OVERHEAD_BYTES)
}

impl LayoutCache {
    fn new() -> Self {
        Self::with_limits(MAX_CACHE_ENTRIES, MAX_CACHE_BYTES)
    }

    fn with_limits(max_entries: usize, max_bytes: usize) -> Self {
        let entries = HashMap::with_capacity(max_entries);
        let table_capacity_high_water = entries.capacity();
        let base_bytes = std::mem::size_of::<Self>().saturating_add(estimated_hash_table_bytes(table_capacity_high_water));
        Self { entries, total_bytes: base_bytes, base_bytes, table_capacity_high_water, clock: 0, max_entries, max_bytes }
    }

    fn get(&mut self, key: &dyn CacheKeyData) -> Option<CachedPlan> {
        self.clock = self.clock.saturating_add(1);
        let last_used = self.clock;
        let plan = self.entries.get_mut(key)?;
        plan.last_used = last_used;
        Some(plan.clone())
    }

    fn insert(&mut self, key: Arc<CacheKey>, plan: CachedPlan) {
        let mut plan = plan;
        plan.key_hash = key.view().request_hash();
        plan.accounted_bytes = estimated_key_allocation_bytes(&key).saturating_add(estimated_plan_allocation_bytes(&plan));
        if self.base_bytes.saturating_add(plan.accounted_bytes) > self.max_bytes {
            return;
        }
        if let Some(old) = self.entries.remove(key.as_ref()) {
            self.total_bytes = self.total_bytes.saturating_sub(old.accounted_bytes);
        }

        while self.entries.len() >= self.max_entries || self.total_bytes.saturating_add(plan.accounted_bytes) > self.max_bytes {
            if !self.evict_lru_entry() {
                return;
            }
        }

        if self.total_bytes.saturating_add(plan.accounted_bytes) > self.max_bytes {
            return;
        }
        self.clock = self.clock.saturating_add(1);
        plan.last_used = self.clock;
        self.total_bytes = self.total_bytes.saturating_add(plan.accounted_bytes);
        self.entries.insert(key, plan);
        self.update_table_allocation_accounting();
        self.compact_table_if_base_exceeds_limit();

        // An insertion can rehash a tombstone-heavy table even when the live entry count is
        // bounded. Charge that retained growth and evict additional plans before returning.
        while self.entries.len() > self.max_entries || self.total_bytes > self.max_bytes {
            if !self.evict_lru_entry() {
                break;
            }
        }
    }

    fn update_table_allocation_accounting(&mut self) {
        let observed_capacity = self.entries.capacity();
        if observed_capacity <= self.table_capacity_high_water {
            return;
        }

        // HashMap::capacity() can decrease after removals because tombstones consume reusable
        // slots even though the larger bucket allocation remains retained. Track the greatest
        // capacity observed after insertion and never lower the accounted table estimate.
        let old_table_bytes = estimated_hash_table_bytes(self.table_capacity_high_water);
        let new_table_bytes = estimated_hash_table_bytes(observed_capacity);
        let growth_bytes = new_table_bytes.saturating_sub(old_table_bytes);
        self.table_capacity_high_water = observed_capacity;
        self.base_bytes = self.base_bytes.saturating_add(growth_bytes);
        self.total_bytes = self.total_bytes.saturating_add(growth_bytes);
    }

    fn compact_table_if_base_exceeds_limit(&mut self) {
        if self.base_bytes <= self.max_bytes {
            return;
        }

        // A table allocation can eventually outgrow the whole cache budget after extreme churn.
        // Rebuild only in that case so the oversized allocation is actually released; evicting
        // entries alone cannot make room when the table itself exceeds the cap.
        let mut compacted = HashMap::with_capacity(self.max_entries);
        compacted.extend(self.entries.drain());
        self.entries = compacted;
        self.table_capacity_high_water = self.entries.capacity();
        self.base_bytes = std::mem::size_of::<Self>().saturating_add(estimated_hash_table_bytes(self.table_capacity_high_water));
        let retained_entry_bytes = self.entries.values().map(|plan| plan.accounted_bytes).fold(0usize, usize::saturating_add);
        self.total_bytes = self.base_bytes.saturating_add(retained_entry_bytes);
    }

    fn evict_lru_entry(&mut self) -> bool {
        let Some(lru_key) = self.entries.iter().min_by_key(|(_, entry)| entry.last_used).map(|(key, _)| Arc::clone(key)) else {
            return false;
        };
        let Some(evicted) = self.entries.remove(&lru_key) else {
            return false;
        };
        self.total_bytes = self.total_bytes.saturating_sub(evicted.accounted_bytes);
        true
    }

    fn clear_context(&mut self, context_id: u64) {
        let mut released_bytes = 0usize;
        self.entries.retain(|key, plan| {
            if key.context_id == context_id {
                released_bytes = released_bytes.saturating_add(plan.accounted_bytes);
                false
            } else {
                true
            }
        });
        self.total_bytes = self.total_bytes.saturating_sub(released_bytes).max(self.base_bytes);
    }
}

static CACHE: Mutex<Option<LayoutCache>> = Mutex::new(None);

/// Pure layout algorithm: selects line break boundaries for `text` given `avail_width`.
/// Uses `measure_fn` to measure candidate substring widths.
pub fn select_line_ranges<F>(text: &str, avail_width: f32, mut measure_fn: F) -> Result<Vec<TextLineRange>, FallbackReason>
where
    F: FnMut(&str) -> Result<f32, FallbackReason>,
{
    select_line_ranges_with(text, avail_width, &mut measure_fn, || {})
}

fn select_line_ranges_with<F, S>(
    text: &str,
    avail_width: f32,
    mut measure_fn: F,
    mut record_segmentation: S,
) -> Result<Vec<TextLineRange>, FallbackReason>
where
    F: FnMut(&str) -> Result<f32, FallbackReason>,
    S: FnMut(),
{
    if !avail_width.is_finite() || avail_width < 1.0 {
        return Err(FallbackReason::InvalidWidth);
    }

    let scalar_count = text.chars().take(MAX_THAI_SCALARS.saturating_add(1)).count();
    if scalar_count > MAX_THAI_SCALARS {
        return Err(FallbackReason::TextTooLong(scalar_count));
    }

    let hard_line_count = text.as_bytes().iter().filter(|&&byte| byte == b'\n').count().saturating_add(1);
    if hard_line_count > MAX_CELL_LAYOUT_LINES {
        return Err(FallbackReason::BudgetExhausted);
    }

    let hard_lines = split_hard_breaks(text);
    let mut out_ranges = Vec::with_capacity(hard_line_count);
    let mut cell_measurements = 0usize;
    let mut cell_measured_bytes = 0usize;

    for (content_range, full_range, has_hard_break) in hard_lines {
        let para_content = text.get(content_range.clone()).unwrap_or("");
        if para_content.is_empty() {
            push_line_range(
                &mut out_ranges,
                TextLineRange {
                    source_range: full_range,
                    paint_range: content_range,
                    break_kind: if has_hard_break { Some(BreakKind::HardBreak) } else { None },
                },
            )?;
            continue;
        }

        let breaks = get_break_opportunities_with(para_content, &mut record_segmentation);
        let mut line_start_local = 0usize;
        let mut candidate_index = breaks.partition_point(|&offset| offset <= line_start_local);

        while line_start_local < para_content.len() {
            let Some(first_candidate) = breaks.get(candidate_index).copied() else {
                return Err(FallbackReason::BudgetExhausted);
            };

            let mut best_fit: Option<(usize, usize, usize)> = None;

            while let Some(cand_local) = breaks.get(candidate_index).copied() {
                let Some(next_measurements) = cell_measurements.checked_add(1) else {
                    return Err(FallbackReason::BudgetExhausted);
                };
                cell_measurements = next_measurements;
                if cell_measurements > MAX_CELL_MEASUREMENTS {
                    return Err(FallbackReason::BudgetExhausted);
                }

                let cand_bytes = cand_local.saturating_sub(line_start_local);
                let Some(next_measured_bytes) = cell_measured_bytes.checked_add(cand_bytes) else {
                    return Err(FallbackReason::BudgetExhausted);
                };
                cell_measured_bytes = next_measured_bytes;
                if cell_measured_bytes > MAX_CELL_MEASURED_BYTES {
                    return Err(FallbackReason::BudgetExhausted);
                }

                let candidate_slice = para_content.get(line_start_local..cand_local).unwrap_or("");
                let paint_trimmed = candidate_slice.trim_end_matches([' ', '\t']);
                let paint_len = paint_trimmed.len();
                let cand_paint_end = line_start_local.saturating_add(paint_len);

                let w = measure_fn(paint_trimmed)?;

                if w <= avail_width {
                    best_fit = Some((cand_local, cand_paint_end, candidate_index));
                    candidate_index = candidate_index.saturating_add(1);
                } else {
                    break;
                }
            }

            let (chosen_break_local, chosen_paint_end_local, chosen_index) = match best_fit {
                Some((b, p, i)) => (b, p, i),
                None => {
                    let cand_slice = para_content.get(line_start_local..first_candidate).unwrap_or("");
                    let paint_len = cand_slice.trim_end_matches([' ', '\t']).len();
                    (first_candidate, line_start_local.saturating_add(paint_len), candidate_index)
                }
            };
            if chosen_break_local <= line_start_local {
                return Err(FallbackReason::BudgetExhausted);
            }
            let is_soft = chosen_break_local < para_content.len();

            let global_line_start = content_range.start.saturating_add(line_start_local);
            let global_line_end = content_range.start.saturating_add(chosen_break_local);
            let global_paint_end = content_range.start.saturating_add(chosen_paint_end_local);

            let (source_range, break_kind) = if chosen_break_local == para_content.len() {
                // End of this paragraph: include hard break bytes in source_range if present
                let final_end = if has_hard_break { full_range.end } else { global_line_end };
                (global_line_start..final_end, if has_hard_break { Some(BreakKind::HardBreak) } else { None })
            } else {
                (global_line_start..global_line_end, if is_soft { Some(BreakKind::SoftWrap) } else { None })
            };

            push_line_range(&mut out_ranges, TextLineRange { source_range, paint_range: global_line_start..global_paint_end, break_kind })?;

            line_start_local = chosen_break_local;
            candidate_index = chosen_index.saturating_add(1);
        }
    }

    Ok(out_ranges)
}

fn push_line_range(out: &mut Vec<TextLineRange>, range: TextLineRange) -> Result<(), FallbackReason> {
    if out.len() >= MAX_CELL_LAYOUT_LINES {
        return Err(FallbackReason::BudgetExhausted);
    }
    out.push(range);
    Ok(())
}

/// Computes the available horizontal width for the Thai renderer, accounting for decorations,
/// padding, and the alignment-specific indent.
pub(crate) fn available_text_width(rect: Rect, text_left: f32, pad: f32, indent: f32, halign: HAlign) -> f32 {
    let content_left = if text_left.is_finite() { text_left.max(rect.left()).min(rect.right()) } else { rect.left() };
    let indent = if matches!(halign, HAlign::Left | HAlign::Right | HAlign::General) { indent.max(0.0) } else { 0.0 };
    (rect.right() - pad - indent - (content_left + pad)).max(4.0)
}

fn line_x(rect: Rect, content_left: f32, halign: HAlign, pad: f32, indent: f32, line_width: f32) -> f32 {
    let line_indent = if matches!(halign, HAlign::Left | HAlign::Right | HAlign::General) { indent.max(0.0) } else { 0.0 };
    match halign {
        HAlign::Center => ((content_left + pad) + (rect.right() - pad) - line_width) / 2.0,
        HAlign::Right => rect.right() - pad - line_indent - line_width,
        _ => content_left + pad + line_indent,
    }
}

fn aggregate_layout_size(lines: &[ThaiLayoutLine]) -> Vec2 {
    let mut total_h = 0.0f32;
    let mut max_w = 0.0f32;
    for line in lines {
        let size = line.galley.size();
        total_h += size.y;
        if size.x > max_w {
            max_w = size.x;
        }
    }
    vec2(max_w, total_h)
}

/// Executes Thai layout for a cell, returning the ready layout or fallback reason.
pub fn layout_cell(p: &Painter, text: &str, font: &FontId, color: Color32, avail_width: f32, halign: HAlign, zoom: f32) -> ThaiLayoutResult {
    if !avail_width.is_finite() || avail_width < 1.0 || !zoom.is_finite() || zoom <= 0.0 {
        return ThaiLayoutResult::LegacyFallback(FallbackReason::InvalidWidth);
    }
    // Bound lookup hashing even for hostile input. Valid cached text was scalar-checked
    // on its first miss, so warm hits need no scalar scan or owned key allocation.
    if text.len() > MAX_THAI_SCALARS.saturating_mul(4) {
        return ThaiLayoutResult::LegacyFallback(FallbackReason::TextTooLong(MAX_THAI_SCALARS + 1));
    }
    let (budget_id, pass_nr) = frame_budget_key(p.ctx());
    let pixels_per_point_bits = p.ctx().pixels_per_point().to_bits();
    let request = p.ctx().data_mut(|data| {
        let identity = *data.get_temp_mut_or_insert_with(context_cache_identity_id(), new_context_cache_identity);
        let key = CacheKeyRef {
            text,
            family_name: match &font.family {
                egui::FontFamily::Proportional => "Proportional",
                egui::FontFamily::Monospace => "Monospace",
                egui::FontFamily::Name(name) => name.as_ref(),
            },
            context_id: identity.context_id,
            font_size_bits: font.size.to_bits(),
            avail_width_bits: avail_width.to_bits(),
            pixels_per_point_bits,
            font_gen: identity.font_generation,
            zoom_pct: (zoom * 100.0).clamp(0.0, 65535.0) as u16,
            halign: match halign {
                HAlign::Center => 1,
                HAlign::Right => 2,
                _ => 0,
            },
        };
        // Lock order is Context data -> cache, never the reverse. Both locks are
        // released before any egui font measurement or galley construction.
        let cached_plan = CACHE.lock().ok().and_then(|mut guard| guard.get_or_insert_with(LayoutCache::new).get(&key));
        if cached_plan.is_none() {
            let count = text.chars().take(MAX_THAI_SCALARS + 1).count();
            if count > MAX_THAI_SCALARS {
                return Err(FallbackReason::TextTooLong(count));
            }
        }
        let state = data.get_temp_mut_or_insert_with(budget_id, || FrameBudgetState::new(pass_nr));
        if state.pass_nr != pass_nr {
            state.start_pass(pass_nr);
        }
        let key_hash = cached_plan.as_ref().map_or_else(|| key.request_hash(), |plan| plan.key_hash);
        state.record_request(key_hash, cached_plan.is_some());
        if let Some(plan) = cached_plan {
            Ok(CacheRequest::Cached(plan))
        } else if state.exhausted || state.suspend_misses {
            state.exhausted = true;
            Ok(CacheRequest::Deferred)
        } else {
            Ok(CacheRequest::Measure(Arc::new(key.into_owned())))
        }
    });
    let request = match request {
        Ok(request) => request,
        Err(reason) => return ThaiLayoutResult::LegacyFallback(reason),
    };
    let cache_key = match request {
        CacheRequest::Deferred => return ThaiLayoutResult::LegacyFallback(FallbackReason::FrameBudgetExhausted),
        CacheRequest::Measure(key) => key,
        CacheRequest::Cached(plan) => {
            if let Some(reason) = plan.fallback_reason {
                return ThaiLayoutResult::LegacyFallback(reason);
            }
            let mut lines = Vec::with_capacity(plan.lines.len());
            for line in plan.lines.iter() {
                let paint_text = text.get(line.range.paint_range.clone()).unwrap_or("");
                let galley = p.layout_no_wrap(paint_text.to_string(), font.clone(), color);
                lines.push(ThaiLayoutLine { range: line.range.clone(), galley });
            }
            let size = aggregate_layout_size(&lines);
            return ThaiLayoutResult::Ready(ThaiCellLayout { lines, size });
        }
    };

    // Cache miss: run layout with measurement
    let ranges_res = select_line_ranges_with(
        text,
        avail_width,
        |slice| {
            if !reserve_frame_measurement(p.ctx(), slice.len()) {
                return Err(FallbackReason::FrameBudgetExhausted);
            }
            let g = p.layout_no_wrap(slice.to_string(), font.clone(), color);
            Ok(g.size().x)
        },
        || record_segmentation(p.ctx()),
    );

    let ranges = match ranges_res {
        Ok(r) => r,
        Err(FallbackReason::FrameBudgetExhausted) => {
            mark_frame_budget_exhausted(p.ctx());
            return ThaiLayoutResult::LegacyFallback(FallbackReason::FrameBudgetExhausted);
        }
        Err(reason) => {
            let plan = CachedPlan { lines: Arc::from([]), key_hash: 0, fallback_reason: Some(reason), accounted_bytes: 0, last_used: 0 };
            if let Ok(mut cache_guard) = CACHE.lock() {
                cache_guard.get_or_insert_with(LayoutCache::new).insert(cache_key, plan);
            }
            return ThaiLayoutResult::LegacyFallback(reason);
        }
    };

    let mut lines = Vec::with_capacity(ranges.len());
    let mut plan_lines = Vec::with_capacity(ranges.len());
    for r in ranges {
        let paint_text = text.get(r.paint_range.clone()).unwrap_or("");
        let galley = p.layout_no_wrap(paint_text.to_string(), font.clone(), color);
        plan_lines.push(CachedPlanLine { range: r.clone() });
        lines.push(ThaiLayoutLine { range: r, galley });
    }

    let total_size = aggregate_layout_size(&lines);
    let plan = CachedPlan { lines: plan_lines.into(), key_hash: 0, fallback_reason: None, accounted_bytes: 0, last_used: 0 };

    if let Ok(mut cache_guard) = CACHE.lock() {
        let cache = cache_guard.get_or_insert_with(LayoutCache::new);
        cache.insert(cache_key, plan);
    }

    ThaiLayoutResult::Ready(ThaiCellLayout { lines, size: total_size })
}

/// Cell geometry and appearance used to paint an already measured Thai layout.
pub struct ThaiCellPaint {
    pub rect: Rect,
    pub text_left: f32,
    pub halign: HAlign,
    pub valign: VAlign,
    pub pad: f32,
    pub indent: f32,
    pub z: f32,
    pub color: Color32,
}

/// Paints the ready Thai cell layout respecting alignment, padding, and clipping.
pub fn paint_thai_cell(p: &Painter, layout: &ThaiCellLayout, paint: ThaiCellPaint) {
    let ThaiCellPaint { rect, text_left, halign, valign, pad, indent, z, color } = paint;
    if layout.lines.is_empty() {
        return;
    }

    let content_left = if text_left.is_finite() { text_left.max(rect.left()).min(rect.right()) } else { rect.left() };
    let clip_rect = Rect::from_min_max(pos2(content_left, rect.top()), rect.max);
    let clip = clip_rect.intersect(p.clip_rect());
    let painter = p.with_clip_rect(clip);

    let pad_v = 1.5 * z;
    let y_top = rect.top() + pad_v;
    let y_bottom = rect.bottom() - layout.size.y - pad_v;
    let mut cur_y = match valign {
        VAlign::Top => y_top,
        VAlign::Bottom => y_bottom,
        _ => (y_top + y_bottom) / 2.0,
    };

    for line in &layout.lines {
        let line_w = line.galley.size().x;
        let line_h = line.galley.size().y;
        let x = line_x(rect, content_left, halign, pad, indent, line_w);
        painter.galley(pos2(x, cur_y), line.galley.clone(), color);
        cur_y += line_h;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::FontFamily;
    use sha2::{Digest, Sha256};

    const CACHE_TEST_FONT_FAMILY: &str = "gridcraft-cache-test-family";
    const REQUIRED_THAI_TEST_FAMILY: &str = "gridcraft-required-thai-test-face";
    const PINNED_NOTO_SANS_THAI_SHA256: &str = "5a1c559bb539583c8a1fd99d1c5b9491e5e14478c9cd2bd0970d5c3096cc9ef8";
    static CACHE_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn cache_test_font_definitions(monospaced: bool) -> egui::FontDefinitions {
        let mut definitions = crate::theme::pinned_font_definitions();
        let fonts_root = std::env::var_os("CRAFT_FONTS_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../craft-fonts"));
        let path = fonts_root.join("fonts/noto-sans-thai/NotoSansThai.ttf");
        let bytes = std::fs::read(&path).unwrap_or_else(|error| {
            panic!("required pinned Noto Sans Thai font could not be read at {}: {error}; set CRAFT_FONTS_DIR", path.display())
        });
        let sha256 = Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect::<String>();
        assert_eq!(
            sha256,
            PINNED_NOTO_SANS_THAI_SHA256,
            "required Noto Sans Thai font at {} has the wrong SHA-256; expected {}, found {}",
            path.display(),
            PINNED_NOTO_SANS_THAI_SHA256,
            sha256
        );
        let key = "NotoSansThai".to_string();
        definitions.font_data.insert(key.clone(), Arc::new(egui::FontData::from_owned(bytes)));
        for family in [
            FontFamily::Proportional,
            FontFamily::Monospace,
            FontFamily::Name(crate::theme::CELL.into()),
            FontFamily::Name(crate::theme::CELL_BOLD.into()),
            FontFamily::Name(crate::theme::UI.into()),
        ] {
            if let Some(chain) = definitions.families.get_mut(&family)
                && !chain.contains(&key)
            {
                chain.push(key.clone());
            }
        }
        definitions.families.insert(FontFamily::Name(REQUIRED_THAI_TEST_FAMILY.into()), vec![key]);
        let source_family = if monospaced { FontFamily::Monospace } else { FontFamily::Proportional };
        if let Some(chain) = definitions.families.get(&source_family).cloned() {
            definitions.families.insert(FontFamily::Name(CACHE_TEST_FONT_FAMILY.into()), chain);
        }
        definitions
    }

    fn assert_required_thai_face(ctx: &egui::Context) {
        let font = FontId::new(16.0, FontFamily::Name(REQUIRED_THAI_TEST_FAMILY.into()));
        for ch in "กขไทยภาษาเก้า".chars() {
            let mut has_glyph = false;
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                has_glyph = ui.fonts_mut(|fonts| fonts.has_glyph(&font, ch));
            });
            output.textures_delta.clear();
            assert!(has_glyph, "pinned Noto Sans Thai face has no glyph for {ch:?}");
        }
    }

    fn install_cache_test_fonts(ctx: &egui::Context, monospaced: bool) {
        crate::theme::install_fonts(ctx, cache_test_font_definitions(monospaced));
        assert_required_thai_face(ctx);
    }

    fn set_cache_test_fonts(ctx: &egui::Context, monospaced: bool) {
        ctx.set_fonts(cache_test_font_definitions(monospaced));
        assert_required_thai_face(ctx);
    }

    fn raw_ranges(ctx: &egui::Context, text: &str, width: f32) -> Vec<TextLineRange> {
        let font = FontId::new(15.0, FontFamily::Name(CACHE_TEST_FONT_FAMILY.into()));
        let mut result = Ok(Vec::new());
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            begin_frame_budget(ui.ctx());
            result =
                select_line_ranges(text, width, |slice| Ok(ui.painter().layout_no_wrap(slice.to_owned(), font.clone(), Color32::WHITE).size().x));
        });
        output.textures_delta.clear();
        result.expect("raw range calculation should succeed")
    }

    fn cached_ranges(ctx: &egui::Context, text: &str, width: f32) -> Vec<TextLineRange> {
        let font = FontId::new(15.0, FontFamily::Name(CACHE_TEST_FONT_FAMILY.into()));
        let mut ranges = Vec::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            begin_frame_budget(ui.ctx());
            if let ThaiLayoutResult::Ready(layout) = layout_cell(ui.painter(), text, &font, Color32::WHITE, width, HAlign::Left, 1.0) {
                let mut total_h = 0.0f32;
                let mut max_w = 0.0f32;
                for line in &layout.lines {
                    let size = line.galley.size();
                    total_h += size.y;
                    if size.x > max_w {
                        max_w = size.x;
                    }
                }
                assert_eq!(layout.size, vec2(max_w, total_h), "aggregate size must match the final galleys returned for this context");
                ranges = layout.lines.into_iter().map(|line| line.range).collect();
            }
        });
        output.textures_delta.clear();
        ranges
    }

    fn find_font_sensitive_width(text: &str) -> Option<(f32, Vec<TextLineRange>, Vec<TextLineRange>)> {
        let proportional = egui::Context::default();
        set_cache_test_fonts(&proportional, false);
        let monospaced = egui::Context::default();
        set_cache_test_fonts(&monospaced, true);
        for width in (8..=240).map(|value| value as f32) {
            let proportional_ranges = raw_ranges(&proportional, text, width);
            let monospaced_ranges = raw_ranges(&monospaced, text, width);
            if proportional_ranges != monospaced_ranges {
                return Some((width, proportional_ranges, monospaced_ranges));
            }
        }
        None
    }

    fn cache_entry_count_for_text(text: &str) -> usize {
        CACHE.lock().ok().and_then(|guard| guard.as_ref().map(|cache| cache.entries.keys().filter(|key| key.text == text).count())).unwrap_or(0)
    }

    fn set_test_cache_limits(max_entries: usize, max_bytes: usize) {
        if let Ok(mut cache) = CACHE.lock() {
            *cache = Some(LayoutCache::with_limits(max_entries, max_bytes));
        }
    }

    fn retry_repaint_request_count(ctx: &egui::Context) -> usize {
        ctx.repaint_causes().iter().filter(|cause| cause.file.ends_with("thai_layout.rs")).count()
    }

    fn layout_test_pass(ctx: &egui::Context, text: &str, widths: &[f32]) -> (Vec<bool>, FrameBudgetUsage, bool, usize) {
        let font = FontId::new(15.0, FontFamily::Proportional);
        let mut ready = Vec::new();
        let mut usage = FrameBudgetUsage::default();
        let previous_retry_requests = std::cell::Cell::new(0usize);
        let scope_split = widths.len() / 2;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            previous_retry_requests.set(retry_repaint_request_count(ui.ctx()));
            begin_frame_budget(ui.ctx());
            for width in &widths[..scope_split] {
                ready.push(matches!(layout_cell(ui.painter(), text, &font, Color32::WHITE, *width, HAlign::Left, 1.0), ThaiLayoutResult::Ready(_)));
            }
            let _ = finish_frame_budget(ui.ctx());
            // A second paint scope in this same egui pass shares the first scope's allowance.
            begin_frame_budget(ui.ctx());
            for width in &widths[scope_split..] {
                ready.push(matches!(layout_cell(ui.painter(), text, &font, Color32::WHITE, *width, HAlign::Left, 1.0), ThaiLayoutResult::Ready(_)));
            }
            let _ = finish_frame_budget(ui.ctx());
            usage = frame_budget_usage(ui.ctx());
        });
        output.textures_delta.clear();
        (ready, usage, last_pass_requested_retry(ctx), previous_retry_requests.get())
    }

    fn empty_pass_retry_request_count(ctx: &egui::Context) -> usize {
        let previous_retry_requests = std::cell::Cell::new(0usize);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| previous_retry_requests.set(retry_repaint_request_count(ui.ctx())));
        output.textures_delta.clear();
        previous_retry_requests.get()
    }

    fn last_pass_requested_retry(ctx: &egui::Context) -> bool {
        let (id, _) = frame_budget_key(ctx);
        ctx.data(|data| data.get_temp::<FrameBudgetState>(id).is_some_and(|state| state.repaint_retry))
    }

    fn retry_test_key(text: &str) -> CacheKey {
        CacheKey {
            text: text.to_string(),
            family_name: "retry-test-family".to_string(),
            context_id: 77,
            font_size_bits: 15.0f32.to_bits(),
            avail_width_bits: 80.0f32.to_bits(),
            pixels_per_point_bits: 1.0f32.to_bits(),
            font_gen: 1,
            zoom_pct: 100,
            halign: 0,
        }
    }

    #[test]
    fn test_has_thai_chars() {
        assert!(has_thai_chars("สวัสดี"));
        assert!(has_thai_chars("Hello สวัสดี"));
        assert!(has_thai_chars("123 บาท"));
        assert!(!has_thai_chars("Hello World"));
        assert!(!has_thai_chars("123.45"));
        assert!(!has_thai_chars(""));
    }

    #[test]
    fn test_hard_breaks_reconstruction() {
        let cases = [
            "สวัสดี",
            "สวัสดี\n",
            "สวัสดี\r\n",
            "สวัสดี\nชาวโลก",
            "สวัสดี\r\nชาวโลก\r\n",
            "\n\n\n",
            "\r\n\r\n",
            "ข้อความ\n\n",
            "ข้อความ\r\n\r\n",
            "ข้อความ\n\r\nท้าย\n",
            "",
            "บรรทัดหนึ่ง\nบรรทัดสอง\nบรรทัดสาม",
        ];

        for text in cases {
            let parts = split_hard_breaks(text);
            let mut reconstructed = String::new();
            for (_, full, _) in &parts {
                reconstructed.push_str(&text[full.clone()]);
            }
            assert_eq!(reconstructed, text, "Full source reconstruction failed for {text:?}");
        }
    }

    #[test]
    fn test_thai_word_breaks_human_verified() {
        // "วันนี้อากาศดีมาก" consists of "วันนี้" (today) + "อากาศ" (weather) + "ดีมาก" (very good)
        let text = "วันนี้อากาศดีมาก";
        let breaks = get_break_opportunities(text);
        assert!(breaks.contains(&0));
        assert!(breaks.contains(&text.len()));
        // The human-checked words are วันนี้ (18 bytes), อากาศ (15 bytes), and ดีมาก.
        assert!(breaks.contains(&18), "Missing expected break at วันนี้ boundary in {breaks:?}");
        assert!(breaks.contains(&33), "Missing expected break at อากาศ boundary in {breaks:?}");
        assert!(breaks.contains(&text.len()), "Missing expected break at final word boundary in {breaks:?}");
    }

    #[test]
    fn test_source_range_exact_concat_invariant() {
        let texts = [
            "วันนี้อากาศดีมาก",
            "เก้าอี้ น้ำ กำลัง ซื้อ เสื้อ เผื่อ",
            "ยอดรวม Total 1,234.50 บาท (VAT 7%) + ฿",
            "ข้อความไทย\nบรรทัดสอง\r\nบรรทัดสามและสี่",
            "ข้อความไทย\n",
            "ข้อความไทย\r\n",
            "ข้อความไทย\n\n",
            "ข้อความไทย\r\n\r\n",
            "ข้อความหนึ่ง\n\r\nบรรทัดท้าย\n",
            "ข้อความไทยยาวมากๆ ที่ต้องตัดคำหลายๆ บรรทัดติดต่อกันอย่างต่อเนื่อง",
            "   ช่องว่างนำหน้า และช่องว่างท้าย   ",
            "คำเดียวที่ยาวมากๆๆๆๆๆๆๆๆๆๆๆๆๆๆๆๆๆๆๆๆๆๆๆๆๆๆ",
        ];

        for text in texts {
            let permitted_soft_breaks: Vec<usize> = split_hard_breaks(text)
                .into_iter()
                .flat_map(|(content_range, _, _)| {
                    let paragraph = text.get(content_range.clone()).unwrap_or("");
                    get_break_opportunities(paragraph).into_iter().map(move |offset| content_range.start.saturating_add(offset))
                })
                .collect();
            for avail_w in [30.0, 50.0, 100.0, 150.0, 500.0] {
                let ranges = select_line_ranges(text, avail_w, |s| {
                    // Simple mock width proportional to character count
                    Ok(s.chars().count() as f32 * 10.0)
                })
                .expect("select_line_ranges must succeed");

                assert!(!ranges.is_empty(), "Ranges must not be empty for text {text:?}");

                let mut concat = String::new();
                let mut prev_end = 0;
                for r in &ranges {
                    assert_eq!(r.source_range.start, prev_end, "Discontinuity in source ranges");
                    assert!(
                        text.is_char_boundary(r.source_range.start) && text.is_char_boundary(r.source_range.end),
                        "source ranges must end at UTF-8 boundaries: {r:?}"
                    );
                    assert!(r.paint_range.start >= r.source_range.start && r.paint_range.start <= r.source_range.end);
                    assert!(r.paint_range.end <= r.source_range.end, "paint ranges must stay inside their source range: {r:?}");
                    assert!(
                        text.is_char_boundary(r.paint_range.start) && text.is_char_boundary(r.paint_range.end),
                        "paint ranges must end at UTF-8 boundaries: {r:?}"
                    );
                    if r.break_kind == Some(BreakKind::SoftWrap) {
                        assert!(
                            permitted_soft_breaks.contains(&r.source_range.end),
                            "Soft break {} is not an allowed opportunity for {text:?}",
                            r.source_range.end
                        );
                    }
                    concat.push_str(&text[r.source_range.clone()]);
                    prev_end = r.source_range.end;
                }
                assert_eq!(concat, text, "Concat of source ranges failed for w={avail_w}");
            }
        }
    }

    #[test]
    fn test_oversized_first_segment_clipped_safely() {
        // Single word wider than cell width
        let text = "น้ำ"; // single unbreakable word
        let avail_w = 2.0; // too narrow even for single word
        let ranges = select_line_ranges(text, avail_w, |_| Ok(60.0)).expect("must handle oversized safely");
        assert_eq!(ranges.len(), 1);
        assert_eq!(&text[ranges[0].source_range.clone()], text);

        // Multi-word where each word and space is wider than cell
        let multi = "วัน วัน วัน";
        let multi_ranges = select_line_ranges(multi, 2.0, |_| Ok(50.0)).expect("multi-word oversized");
        assert_eq!(multi_ranges.len(), 5);
        let mut concat = String::new();
        for r in &multi_ranges {
            concat.push_str(&multi[r.source_range.clone()]);
        }
        assert_eq!(concat, multi);
    }

    #[test]
    fn test_budget_exhaustion_returns_fallback() {
        // Text with over 512 break opportunities to trigger MAX_CELL_MEASUREMENTS budget exhaustion
        let text = "คำ ".repeat(600);
        let res = select_line_ranges(&text, 10000.0, |_| Ok(5.0));
        assert!(matches!(res, Err(FallbackReason::BudgetExhausted)));
    }

    #[test]
    fn test_invalid_widths_return_fallback() {
        assert_eq!(select_line_ranges("สวัสดี", -10.0, |_| Ok(10.0)).unwrap_err(), FallbackReason::InvalidWidth);
        assert_eq!(select_line_ranges("สวัสดี", f32::NAN, |_| Ok(10.0)).unwrap_err(), FallbackReason::InvalidWidth);
        assert_eq!(select_line_ranges("สวัสดี", 0.0, |_| Ok(10.0)).unwrap_err(), FallbackReason::InvalidWidth);
    }

    #[test]
    fn center_across_uses_legacy_layout() {
        let mut style = Style::default();
        style.align.wrap = true;
        style.align.h = HAlign::CenterAcross;
        let value = Value::from("ไทย");
        assert_eq!(is_eligible("ไทย", &style, &value, false), Err(FallbackReason::UnsupportedHAlign));
    }

    #[test]
    fn thai_eligibility_is_limited_to_the_approved_t1_alignment_and_rotation_scope() {
        let value = Value::from("ไทย");
        let mut style = Style::default();
        style.align.wrap = true;

        for h in [HAlign::General, HAlign::Left, HAlign::Center, HAlign::Right] {
            for v in [VAlign::Top, VAlign::Center, VAlign::Bottom] {
                style.align.h = h;
                style.align.v = v;
                style.align.rotation = 0;
                assert_eq!(is_eligible("ไทย", &style, &value, false), Ok(()), "supported {h:?}/{v:?}");
            }
        }

        for h in [HAlign::Fill, HAlign::Justify, HAlign::CenterAcross, HAlign::Distributed] {
            style.align.h = h;
            style.align.v = VAlign::Top;
            assert_eq!(is_eligible("ไทย", &style, &value, false), Err(FallbackReason::UnsupportedHAlign), "unsupported horizontal alignment {h:?}");
        }
        for v in [VAlign::Justify, VAlign::Distributed] {
            style.align.h = HAlign::Left;
            style.align.v = v;
            assert_eq!(is_eligible("ไทย", &style, &value, false), Err(FallbackReason::UnsupportedVAlign), "unsupported vertical alignment {v:?}");
        }

        for rotation in [-90, -45, 1, 45, 90, 255] {
            style.align.h = HAlign::Left;
            style.align.v = VAlign::Top;
            style.align.rotation = rotation;
            assert_eq!(
                is_eligible("ไทย", &style, &value, false),
                Err(FallbackReason::Rotated(rotation)),
                "rotation {rotation} must use legacy rendering"
            );
        }

        style.align.rotation = 0;
        assert_eq!(is_eligible("ไทย", &style, &Value::Number(1.0), false), Err(FallbackReason::NumericOrDate));
        assert_eq!(is_eligible("ไทย", &style, &value, true), Err(FallbackReason::NumericOrDate));
        assert_eq!(is_eligible("ไทย", &style, &Value::from("ไทย"), false), Ok(()), "a text formula result remains eligible");
        style.align.wrap = false;
        assert_eq!(is_eligible("ไทย", &style, &value, false), Err(FallbackReason::WrapDisabled));
    }

    #[test]
    fn too_many_explicit_lines_fall_back_before_allocating_line_plans() {
        let text = "\n".repeat(MAX_CELL_LAYOUT_LINES);
        assert_eq!(select_line_ranges(&text, 100.0, |_| Ok(1.0)).unwrap_err(), FallbackReason::BudgetExhausted);
    }

    #[test]
    fn frame_budget_stops_at_exact_measurement_and_byte_limits() {
        let mut by_count = FrameBudget::default();
        let limits = FrameBudgetLimits::default();
        for _ in 0..MAX_FRAME_MEASUREMENTS {
            assert!(by_count.reserve_measurement(0, limits));
        }
        assert!(!by_count.reserve_measurement(0, limits));

        let mut by_bytes = FrameBudget::default();
        assert!(by_bytes.reserve_measurement(MAX_FRAME_MEASURED_BYTES, limits));
        assert!(!by_bytes.reserve_measurement(1, limits));
        assert_eq!(by_bytes.measured_bytes, MAX_FRAME_MEASURED_BYTES);

        let small_count_limits = FrameBudgetLimits { max_measurements: 1, max_measured_bytes: 8 };
        let mut by_small_count = FrameBudget::default();
        assert!(by_small_count.reserve_measurement(8, small_count_limits));
        assert!(!by_small_count.reserve_measurement(0, small_count_limits));
        assert_eq!(by_small_count.measurements, 1);
        assert_eq!(by_small_count.measured_bytes, 8);

        let small_byte_limits = FrameBudgetLimits { max_measurements: 8, max_measured_bytes: 4 };
        let mut by_small_bytes = FrameBudget::default();
        assert!(!by_small_bytes.reserve_measurement(5, small_byte_limits));
        assert_eq!(by_small_bytes.measurements, 0, "a rejected candidate must not consume a count reservation");
        assert_eq!(by_small_bytes.measured_bytes, 0, "a rejected candidate must not consume byte allowance");
    }

    #[test]
    fn frame_budget_is_isolated_between_contexts_regression() {
        let context_a = egui::Context::default();
        let context_b = egui::Context::default();
        let mut usage_after_context_b = FrameBudgetUsage::default();
        let limits = FrameBudgetLimits { max_measurements: 1, max_measured_bytes: 100 };
        install_frame_budget_finalizer(&context_a);
        install_frame_budget_finalizer(&context_b);
        set_frame_budget_limits(&context_a, limits);
        set_frame_budget_limits(&context_b, limits);
        let mut output = context_a.run_ui(egui::RawInput::default(), |ui| {
            begin_frame_budget(ui.ctx());
            assert!(reserve_frame_measurement(ui.ctx(), 7));
            let mut nested_output = context_b.run_ui(egui::RawInput::default(), |ui_b| {
                begin_frame_budget(ui_b.ctx());
                assert!(reserve_frame_measurement(ui_b.ctx(), 9));
                assert!(!reserve_frame_measurement(ui_b.ctx(), 1));
            });
            nested_output.textures_delta.clear();
            usage_after_context_b = frame_budget_usage(ui.ctx());
            assert_eq!(usage_after_context_b.measurements, 1);
            begin_frame_budget(ui.ctx());
            assert!(!reserve_frame_measurement(ui.ctx(), 1), "repeated paint calls in one pass must share the same allowance");
            mark_frame_budget_exhausted(ui.ctx());
        });
        output.textures_delta.clear();
        assert!(last_pass_requested_retry(&context_a));
        assert_eq!(usage_after_context_b.measurements, 1, "starting a pass in another egui context must not reset this context's budget");

        let mut next_pass_usage = FrameBudgetUsage::default();
        let mut next_output = context_a.run_ui(egui::RawInput::default(), |ui| {
            begin_frame_budget(ui.ctx());
            next_pass_usage = frame_budget_usage(ui.ctx());
            assert_eq!(next_pass_usage.measurements, 0, "a new egui pass resets usage exactly once");
            assert!(reserve_frame_measurement(ui.ctx(), 5));
        });
        next_output.textures_delta.clear();
    }

    #[test]
    fn multiple_paint_scopes_settle_retry_across_actual_passes_regression() {
        let ctx = egui::Context::default();
        install_frame_budget_finalizer(&ctx);
        let key_a = retry_test_key("scope A");
        let key_b = retry_test_key("scope B");

        let mut final_decisions = Vec::new();
        let mut scope_decisions = Vec::new();
        let mut emitted_before_pass = Vec::new();
        for _ in 0..3 {
            let previous_retry_requests = std::cell::Cell::new(0usize);
            let first_scope_decision = std::cell::Cell::new(false);
            let second_scope_decision = std::cell::Cell::new(false);
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                previous_retry_requests.set(retry_repaint_request_count(ui.ctx()));
                begin_frame_budget(ui.ctx());
                record_layout_request(ui.ctx(), &key_a, false);
                mark_frame_budget_exhausted(ui.ctx());
                first_scope_decision.set(finish_frame_budget(ui.ctx()));

                // New work arrives between scope finishes; only end-pass may emit repaint.
                begin_frame_budget(ui.ctx());
                record_layout_request(ui.ctx(), &key_b, false);
                second_scope_decision.set(finish_frame_budget(ui.ctx()));
            });
            output.textures_delta.clear();
            final_decisions.push(last_pass_requested_retry(&ctx));
            scope_decisions.push((first_scope_decision.get(), second_scope_decision.get()));
            emitted_before_pass.push(previous_retry_requests.get());
        }

        assert_eq!(
            emitted_before_pass,
            [0, 1, 1],
            "each exhausted actual pass emits one retry repaint, including contributions from both paint scopes"
        );
        assert!(final_decisions[0]);
        assert!(final_decisions[1]);
        assert!(!final_decisions[2], "stable two-scope work with no cache progress must stop retry repaint");
        assert_eq!(
            scope_decisions,
            [(true, true), (true, true), (true, false)],
            "each scope finish must recompute from the preceding actual pass, and the full final signature wins"
        );
        assert_eq!(empty_pass_retry_request_count(&ctx), 0, "the stopped pass must not leave a repaint request pending");
    }

    #[test]
    fn frame_budget_rejects_an_oversized_candidate_before_egui_measurement() {
        let _lock = CACHE_TEST_LOCK.lock().expect("cache test lock");
        if let Ok(mut cache) = CACHE.lock() {
            *cache = None;
        }
        let ctx = egui::Context::default();
        install_cache_test_fonts(&ctx, false);
        set_frame_budget_limits(&ctx, FrameBudgetLimits { max_measurements: 1, max_measured_bytes: 1 });

        let mut result = ThaiLayoutResult::LegacyFallback(FallbackReason::NoThai);
        let mut usage = FrameBudgetUsage::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            begin_frame_budget(ui.ctx());
            result = layout_cell(ui.painter(), "ไทย", &FontId::proportional(15.0), Color32::WHITE, 100.0, HAlign::Left, 1.0);
            usage = frame_budget_usage(ui.ctx());
        });
        output.textures_delta.clear();
        let repaint = last_pass_requested_retry(&ctx);
        assert!(matches!(result, ThaiLayoutResult::LegacyFallback(FallbackReason::FrameBudgetExhausted)));
        assert_eq!(usage.measurements, 0, "the oversized first candidate is rejected before measurement");
        assert_eq!(usage.measured_bytes, 0, "the failed reservation leaves byte usage unchanged");
        assert!(repaint, "transient pass exhaustion must request a retry");
    }

    #[test]
    fn cache_fitting_work_completes_across_passes_without_starvation() {
        let _lock = CACHE_TEST_LOCK.lock().expect("cache test lock");
        if let Ok(mut cache) = CACHE.lock() {
            *cache = None;
        }
        let ctx = egui::Context::default();
        install_cache_test_fonts(&ctx, false);
        set_frame_budget_limits(&ctx, FrameBudgetLimits { max_measurements: 2, max_measured_bytes: 8 * 1024 });
        let widths: Vec<f32> = (40..45).map(|width| width as f32).collect();

        let (first_ready, first_usage, first_repaint, first_prior_request) = layout_test_pass(&ctx, "ไทย", &widths);
        assert!(first_ready.iter().any(|ready| *ready) && first_ready.iter().any(|ready| !*ready));
        assert!(first_usage.measurements <= 2);
        assert_eq!(first_prior_request, 0);
        assert!(first_repaint);

        let (second_ready, second_usage, second_repaint, second_prior_request) = layout_test_pass(&ctx, "ไทย", &widths);
        assert!(second_ready.iter().any(|ready| *ready) && second_ready.iter().any(|ready| !*ready));
        assert!(second_usage.measurements <= 2);
        assert_eq!(second_prior_request, 1);
        assert!(second_usage.segmentations < widths.len(), "previously successful plans must be reused without segmenting again");
        assert!(second_repaint);

        let (completed_ready, completed_usage, completed_repaint, completed_prior_request) = layout_test_pass(&ctx, "ไทย", &widths);
        assert!(completed_ready.iter().all(|ready| *ready), "all pending cells must finish in finite passes");
        assert_eq!(completed_prior_request, 1);
        assert!(!completed_repaint, "the final warm completion must not schedule another retry");

        let (warm_ready, warm_usage, warm_repaint, warm_prior_request) = layout_test_pass(&ctx, "ไทย", &widths);
        assert!(warm_ready.iter().all(|ready| *ready));
        assert_eq!(warm_prior_request, 0, "no retry repaint may remain after warm completion");
        assert_eq!(warm_usage.segmentations, 0, "warm cache hits must not repeat segmentation");
        assert_eq!(warm_usage.measurements, 0, "warm cache hits must not repeat candidate measurement");
        assert_eq!(warm_usage.measured_bytes, 0);
        assert!(!warm_repaint);
        assert!(completed_usage.segmentations <= widths.len());
    }

    #[test]
    fn no_progress_stops_retry_and_changed_layout_key_restarts_evaluation() {
        let _lock = CACHE_TEST_LOCK.lock().expect("cache test lock");
        if let Ok(mut cache) = CACHE.lock() {
            *cache = None;
        }
        let ctx = egui::Context::default();
        install_cache_test_fonts(&ctx, false);
        set_frame_budget_limits(&ctx, FrameBudgetLimits { max_measurements: 4, max_measured_bytes: 1 });

        let (_, first_usage, first_repaint, first_prior_request) = layout_test_pass(&ctx, "ไทย", &[40.0]);
        let (_, _, second_repaint, second_prior_request) = layout_test_pass(&ctx, "ไทย", &[40.0]);
        let (_, _, third_repaint, third_prior_request) = layout_test_pass(&ctx, "ไทย", &[40.0]);
        assert_eq!(first_usage.measurements, 0);
        assert_eq!([first_prior_request, second_prior_request, third_prior_request], [0, 1, 1]);
        assert!(first_repaint && second_repaint);
        assert!(!third_repaint, "two stable no-progress passes must suppress retry repaint");

        let (_, changed_usage, changed_repaint, changed_prior_request) = layout_test_pass(&ctx, "ไทย", &[41.0]);
        assert_eq!(changed_usage.measurements, 0);
        assert_eq!(changed_prior_request, 0, "retry requests must stop after the no-progress bound");
        assert!(changed_repaint, "a changed width creates a fresh key and must be evaluated again");
    }

    #[test]
    fn stopped_overflow_skips_measurements_on_external_repaints_until_keys_change() {
        let _lock = CACHE_TEST_LOCK.lock().expect("cache test lock");
        set_test_cache_limits(2, 32 * 1024);
        let ctx = egui::Context::default();
        install_cache_test_fonts(&ctx, false);
        set_frame_budget_limits(&ctx, FrameBudgetLimits { max_measurements: 1, max_measured_bytes: 8 * 1024 });
        let widths: Vec<f32> = (40..45).map(|width| width as f32).collect();
        let mut stopped = false;
        for _ in 0..MAX_RETRY_PASSES {
            let (_, _, repaint, _) = layout_test_pass(&ctx, "ไทย", &widths);
            if !repaint {
                stopped = true;
                break;
            }
        }
        assert!(stopped);
        let mut stable_renderers = None;
        for _ in 0..20 {
            let (ready, usage, repaint, prior_requests) = layout_test_pass(&ctx, "ไทย", &widths);
            assert_eq!(usage, FrameBudgetUsage::default(), "an external repaint must not remeasure a stopped workload");
            assert!(!repaint);
            assert_eq!(prior_requests, 0);
            if let Some(previous) = &stable_renderers {
                assert_eq!(&ready, previous, "cached and fallback cells must remain stable");
            } else {
                stable_renderers = Some(ready);
            }
        }
        let changed_widths: Vec<f32> = widths.iter().map(|w| w + 10.0).collect();
        let (_, deferred_usage, restart, _) = layout_test_pass(&ctx, "ไทย", &changed_widths);
        assert!(restart, "changed keys restart evaluation after the complete pass is known");
        assert_eq!(deferred_usage, FrameBudgetUsage::default());
        let (_, restarted_usage, _, _) = layout_test_pass(&ctx, "ไทย", &changed_widths);
        assert!(restarted_usage.measurements > 0);
        if let Ok(mut cache) = CACHE.lock() {
            *cache = None;
        }
    }

    #[test]
    fn borrowed_cache_lookup_preserves_identity_and_shares_plan_lines() {
        let mut cache = LayoutCache::new();
        let key = Arc::new(retry_test_key("ไทย"));
        let lines: Arc<[CachedPlanLine]> =
            Arc::from([CachedPlanLine { range: TextLineRange { source_range: 0..9, paint_range: 0..9, break_kind: None } }]);
        cache
            .insert(Arc::clone(&key), CachedPlan { lines: Arc::clone(&lines), key_hash: 0, fallback_reason: None, accounted_bytes: 0, last_used: 0 });
        let mut owned_hasher = DefaultHasher::new();
        key.hash(&mut owned_hasher);
        assert_eq!(owned_hasher.finish(), key.view().request_hash(), "borrowed and owned lookup hashes must agree");
        for _ in 0..3 {
            let hit = cache.get(&key.view()).expect("borrowed hit");
            assert!(Arc::ptr_eq(&hit.lines, &lines), "hits must share the retained lines allocation");
            assert_eq!(hit.key_hash, key.view().request_hash());
        }
        let mutations: [fn(&mut CacheKey); 9] = [
            |k| k.text.push('!'),
            |k| k.family_name.push('!'),
            |k| k.context_id += 1,
            |k| k.font_size_bits = 16.0f32.to_bits(),
            |k| k.avail_width_bits = 81.0f32.to_bits(),
            |k| k.pixels_per_point_bits = 2.0f32.to_bits(),
            |k| k.font_gen += 1,
            |k| k.zoom_pct += 1,
            |k| k.halign += 1,
        ];
        for mutate in mutations {
            let mut different = key.as_ref().clone();
            mutate(&mut different);
            assert!(cache.get(&different.view()).is_none(), "every layout identity component must distinguish plans");
        }
    }

    #[test]
    fn cache_overflow_working_set_stops_retry_with_bounded_state() {
        let _lock = CACHE_TEST_LOCK.lock().expect("cache test lock");
        set_test_cache_limits(2, 32 * 1024);
        let ctx = egui::Context::default();
        install_cache_test_fonts(&ctx, false);
        set_frame_budget_limits(&ctx, FrameBudgetLimits { max_measurements: 1, max_measured_bytes: 8 * 1024 });
        let widths: Vec<f32> = (40..45).map(|width| width as f32).collect();

        let mut stopped = false;
        let mut retries = 0usize;
        let mut last_decision = false;
        while retries < MAX_RETRY_PASSES {
            let (_, _, repaint, previous_retry_requests) = layout_test_pass(&ctx, "ไทย", &widths);
            assert_eq!(previous_retry_requests, usize::from(last_decision), "the previous actual pass decision must match its emitted retry cause");
            retries = retries.saturating_add(1);
            last_decision = repaint;
            if let Ok(guard) = CACHE.lock()
                && let Some(cache) = guard.as_ref()
            {
                assert!(cache.entries.len() <= 2, "test cache entry limit must remain enforced during overflow");
                assert!(cache.total_bytes <= 32 * 1024, "test cache byte limit must remain enforced during overflow");
            }
            if !repaint {
                stopped = true;
                break;
            }
        }
        assert!(stopped, "an over-capacity working set must stop retry no later than the fixed pass bound");
        assert!(retries <= MAX_RETRY_PASSES);
        assert_eq!(empty_pass_retry_request_count(&ctx), 0, "an overflow stop must leave no Thai retry repaint pending");
        if let Ok(mut cache) = CACHE.lock() {
            *cache = None;
        }
    }

    #[test]
    fn permanent_line_fallback_is_cached_without_retry_repaint() {
        let _lock = CACHE_TEST_LOCK.lock().expect("cache test lock");
        if let Ok(mut cache) = CACHE.lock() {
            *cache = None;
        }
        let ctx = egui::Context::default();
        install_cache_test_fonts(&ctx, false);
        set_frame_budget_limits(&ctx, FrameBudgetLimits { max_measurements: 1, max_measured_bytes: 64 });
        let text = format!("ไทย{}", "\n".repeat(MAX_CELL_LAYOUT_LINES));

        for _ in 0..2 {
            let mut result = ThaiLayoutResult::Ready(ThaiCellLayout { lines: Vec::new(), size: Vec2::ZERO });
            let mut usage = FrameBudgetUsage::default();
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                begin_frame_budget(ui.ctx());
                result = layout_cell(ui.painter(), &text, &FontId::proportional(15.0), Color32::WHITE, 100.0, HAlign::Left, 1.0);
                usage = frame_budget_usage(ui.ctx());
            });
            output.textures_delta.clear();
            let repaint = last_pass_requested_retry(&ctx);
            assert!(matches!(result, ThaiLayoutResult::LegacyFallback(FallbackReason::BudgetExhausted)));
            assert_eq!(usage.segmentations, 0);
            assert_eq!(usage.measurements, 0);
            assert!(!repaint, "permanent cell limits must not request an unbounded retry");
        }
        assert_eq!(cache_entry_count_for_text(&text), 1, "the permanent fallback should be reused from the existing LRU");
    }

    #[test]
    fn thai_geometry_accounts_for_decorations_padding_and_indent() {
        let ctx = egui::Context::default();
        install_cache_test_fonts(&ctx, false);
        let rect = Rect::from_min_max(pos2(10.0, 0.0), pos2(110.0, 30.0));
        let text_left = 25.0;
        assert_eq!(available_text_width(rect, text_left, 5.0, 9.0, HAlign::Left), 66.0);
        assert_eq!(available_text_width(rect, text_left, 5.0, 9.0, HAlign::Center), 75.0);
        assert_eq!(line_x(rect, text_left, HAlign::Left, 5.0, 9.0, 20.0), 39.0);
        assert_eq!(line_x(rect, text_left, HAlign::Right, 5.0, 9.0, 20.0), 76.0);
        assert_eq!(line_x(rect, text_left, HAlign::Center, 5.0, 0.0, 20.0), 57.5);
    }

    #[test]
    fn cache_does_not_share_plans_between_contexts_with_different_fonts() {
        let _lock = CACHE_TEST_LOCK.lock().expect("cache test lock");
        let text = "วันนี้ abcdefghijklmnopqrstuvwxyz วันนี้";
        let (width, proportional_ranges, monospaced_ranges) =
            find_font_sensitive_width(text).expect("pinned proportional and monospace fonts should produce different line plans");
        assert_ne!(proportional_ranges, monospaced_ranges);

        if let Ok(mut cache) = CACHE.lock() {
            *cache = None;
        }
        let proportional = egui::Context::default();
        install_cache_test_fonts(&proportional, false);
        let monospaced = egui::Context::default();
        install_cache_test_fonts(&monospaced, true);

        assert_eq!(cached_ranges(&proportional, text, width), proportional_ranges);
        assert_eq!(
            cached_ranges(&monospaced, text, width),
            monospaced_ranges,
            "a cache entry from another egui context must not supply its break plan"
        );
    }

    #[test]
    fn contexts_without_font_install_wrapper_do_not_share_generation_zero_plans() {
        let _lock = CACHE_TEST_LOCK.lock().expect("cache test lock");
        let text = "วันนี้ no-wrapper context identity ABCDEFGHIJKLMNOP";
        let (width, proportional_ranges, monospaced_ranges) =
            find_font_sensitive_width(text).expect("pinned proportional and monospace fonts should produce different line plans");
        assert_ne!(proportional_ranges, monospaced_ranges);

        if let Ok(mut cache) = CACHE.lock() {
            *cache = None;
        }
        let proportional = egui::Context::default();
        set_cache_test_fonts(&proportional, false);
        let monospaced = egui::Context::default();
        set_cache_test_fonts(&monospaced, true);

        assert_eq!(cached_ranges(&proportional, text, width), proportional_ranges);
        assert_eq!(cache_entry_count_for_text(text), 1);
        assert_eq!(
            cached_ranges(&monospaced, text, width),
            monospaced_ranges,
            "contexts that bypass the font-install helper still need distinct cache identities"
        );
        assert_eq!(cache_entry_count_for_text(text), 2);
    }

    #[test]
    fn changing_one_context_preserves_other_context_plans_and_warm_hits() {
        let _lock = CACHE_TEST_LOCK.lock().expect("cache test lock");
        let text = "วันนี้ context-local font invalidation abcdefghijklmnop";
        let (width, proportional_ranges, monospaced_ranges) =
            find_font_sensitive_width(text).expect("pinned proportional and monospace fonts should produce different line plans");
        assert_ne!(proportional_ranges, monospaced_ranges);

        if let Ok(mut cache) = CACHE.lock() {
            *cache = None;
        }
        let changed = egui::Context::default();
        install_cache_test_fonts(&changed, false);
        let retained = egui::Context::default();
        install_cache_test_fonts(&retained, true);
        assert_eq!(cached_ranges(&changed, text, width), proportional_ranges);
        assert_eq!(cached_ranges(&retained, text, width), monospaced_ranges);
        assert_eq!(cache_entry_count_for_text(text), 2);

        install_cache_test_fonts(&changed, true);
        assert_eq!(cache_entry_count_for_text(text), 1, "replacing fonts should remove only the changed context's prior plan");
        assert_eq!(cached_ranges(&retained, text, width), monospaced_ranges);
        assert_eq!(frame_budget_usage(&retained).measurements, 0, "the unchanged context should retain a warm cache hit");
        assert_eq!(cache_entry_count_for_text(text), 1);
    }

    #[test]
    fn replacing_font_definitions_invalidates_same_text_family_and_width() {
        let _lock = CACHE_TEST_LOCK.lock().expect("cache test lock");
        let text = "วันนี้ abcdefghijklmnopqrstuvwxyz วันนี้";
        let (width, proportional_ranges, monospaced_ranges) =
            find_font_sensitive_width(text).expect("pinned proportional and monospace fonts should produce different line plans");
        assert_ne!(proportional_ranges, monospaced_ranges);

        if let Ok(mut cache) = CACHE.lock() {
            *cache = None;
        }
        let ctx = egui::Context::default();
        install_cache_test_fonts(&ctx, false);
        assert_eq!(cached_ranges(&ctx, text, width), proportional_ranges);
        install_cache_test_fonts(&ctx, true);
        assert_eq!(cached_ranges(&ctx, text, width), monospaced_ranges, "a font rebuild must invalidate the prior plan under the same key fields");
    }

    #[test]
    fn cache_keys_distinguish_dpi_and_reuse_each_scale_after_returning() {
        let _lock = CACHE_TEST_LOCK.lock().expect("cache test lock");
        let text = "วันนี้ DPI cache identity 123456789";
        if let Ok(mut cache) = CACHE.lock() {
            *cache = None;
        }
        let ctx = egui::Context::default();
        install_cache_test_fonts(&ctx, false);
        ctx.set_pixels_per_point(1.0);
        let at_1x = cached_ranges(&ctx, text, 48.0);
        assert_eq!(cache_entry_count_for_text(text), 1);

        ctx.set_pixels_per_point(2.0);
        let at_2x = cached_ranges(&ctx, text, 48.0);
        assert_eq!(cache_entry_count_for_text(text), 2, "DPI changes need a separate cached break plan and metrics");

        ctx.set_pixels_per_point(1.0);
        assert_eq!(cached_ranges(&ctx, text, 48.0), at_1x);
        assert_eq!(cache_entry_count_for_text(text), 2, "returning to 1x should reuse its own prior entry");
        assert!(!at_2x.is_empty());
    }

    #[test]
    fn cache_accounts_for_held_keys_and_rejects_an_oversized_entry() {
        let mut cache = LayoutCache::new();
        assert!(cache.entries.capacity() >= MAX_CACHE_ENTRIES, "the shared LRU should reserve its maximum entry capacity once");
        let minimum_map_bytes = cache.entries.capacity().saturating_mul(std::mem::size_of::<(Arc<CacheKey>, CachedPlan)>());
        assert!(cache.total_bytes >= minimum_map_bytes, "cache accounting must include retained HashMap table capacity");
        let text = "x".repeat(1_000);
        let family_name = "family-name-with-allocation".to_string();
        let key = Arc::new(CacheKey {
            text: text.clone(),
            family_name: family_name.clone(),
            context_id: 1,
            font_size_bits: 12.0f32.to_bits(),
            avail_width_bits: 80.0f32.to_bits(),
            pixels_per_point_bits: 1.0f32.to_bits(),
            font_gen: 1,
            zoom_pct: 100,
            halign: 0,
        });
        let lines = vec![CachedPlanLine { range: TextLineRange { source_range: 0..0, paint_range: 0..0, break_kind: None } }; 16];
        let plan = CachedPlan { lines: lines.into(), key_hash: 0, fallback_reason: None, accounted_bytes: 0, last_used: 0 };
        let expected_entry_bytes = estimated_key_allocation_bytes(&key).saturating_add(estimated_plan_allocation_bytes(&plan));
        cache.insert(Arc::clone(&key), plan);
        let cached = cache.entries.get(key.as_ref()).expect("the ordinary entry should fit");
        assert_eq!(cached.accounted_bytes, expected_entry_bytes, "accounting must include owned key and line-vector capacities");
        assert!(cache.total_bytes >= minimum_map_bytes + expected_entry_bytes);

        let oversized_key = Arc::new(CacheKey {
            text: "x".repeat(MAX_CACHE_BYTES.saturating_add(1)),
            family_name,
            context_id: 1,
            font_size_bits: 12.0f32.to_bits(),
            avail_width_bits: 80.0f32.to_bits(),
            pixels_per_point_bits: 1.0f32.to_bits(),
            font_gen: 1,
            zoom_pct: 100,
            halign: 0,
        });
        let oversized_plan = CachedPlan { lines: Arc::from([]), key_hash: 0, fallback_reason: None, accounted_bytes: 0, last_used: 0 };
        cache.insert(Arc::clone(&oversized_key), oversized_plan);
        assert!(!cache.entries.contains_key(oversized_key.as_ref()), "an oversized entry must never be admitted");
        assert!(cache.total_bytes <= MAX_CACHE_BYTES);
    }

    #[test]
    fn cache_evicts_lru_entries_to_keep_byte_cap() {
        let mut cache = LayoutCache::new();
        let key_for = |index: usize| {
            let text_length = MAX_CACHE_BYTES / 3;
            let mut text = String::with_capacity(text_length.saturating_add(16));
            for _ in 0..text_length {
                text.push('x');
            }
            text.push('-');
            text.push_str(&index.to_string());
            Arc::new(CacheKey {
                text,
                family_name: "family".to_string(),
                context_id: 1,
                font_size_bits: 12.0f32.to_bits(),
                avail_width_bits: 80.0f32.to_bits(),
                pixels_per_point_bits: 1.0f32.to_bits(),
                font_gen: 1,
                zoom_pct: 100,
                halign: 0,
            })
        };
        let key0 = key_for(0);
        let key1 = key_for(1);
        let key2 = key_for(2);
        let empty_plan = || CachedPlan { lines: Arc::from([]), key_hash: 0, fallback_reason: None, accounted_bytes: 0, last_used: 0 };
        cache.insert(Arc::clone(&key0), empty_plan());
        cache.insert(Arc::clone(&key1), empty_plan());
        assert_eq!(cache.entries.len(), 2, "two realistic entries should fit under the byte cap");
        assert!(cache.get(key0.as_ref()).is_some(), "touching the oldest entry should make it most recently used");
        cache.insert(Arc::clone(&key2), empty_plan());

        assert!(cache.total_bytes <= MAX_CACHE_BYTES);
        assert_eq!(cache.entries.len(), 2);
        assert!(cache.entries.contains_key(key0.as_ref()), "the touched entry should survive eviction");
        assert!(!cache.entries.contains_key(key1.as_ref()), "the least recently used entry should be evicted");
        assert!(cache.entries.contains_key(key2.as_ref()), "the newest entry should remain after byte-cap eviction");
    }

    #[test]
    fn cache_accounts_for_table_growth_during_sustained_lru_churn() {
        let mut cache = LayoutCache::new();
        let initial_capacity = cache.entries.capacity();
        let mut high_water_capacity = initial_capacity;
        let mut saw_growth = false;

        for index in 0..50_000usize {
            let mut text = String::with_capacity(8_000);
            text.push_str(&format!("{index:08}"));
            text.extend(std::iter::repeat_n('x', 7_992));
            let key = Arc::new(CacheKey {
                text,
                family_name: "family".to_string(),
                context_id: 1,
                font_size_bits: 12.0f32.to_bits(),
                avail_width_bits: 80.0f32.to_bits(),
                pixels_per_point_bits: 1.0f32.to_bits(),
                font_gen: 1,
                zoom_pct: 100,
                halign: 0,
            });
            cache.insert(key, CachedPlan { lines: Arc::from([]), key_hash: 0, fallback_reason: None, accounted_bytes: 0, last_used: 0 });

            high_water_capacity = high_water_capacity.max(cache.entries.capacity());
            saw_growth |= high_water_capacity > initial_capacity;
            assert!(cache.entries.len() <= MAX_CACHE_ENTRIES, "live entries must remain bounded during churn");
            assert!(cache.total_bytes <= MAX_CACHE_BYTES, "retained accounting must enforce the byte cap during churn");

            if saw_growth {
                // Independently recompute the retained table estimate from the highest capacity
                // observed after an insertion. HashMap::capacity() can fall after removals
                // because tombstones reduce reusable capacity, while the larger table allocation
                // remains held. Do not use LayoutCache::base_bytes or its accounting helper here.
                let expected_bucket_count = high_water_capacity.saturating_mul(8).saturating_add(6) / 7;
                let expected_table_bytes = std::mem::size_of::<LayoutCache>()
                    .saturating_add(expected_bucket_count.saturating_mul(std::mem::size_of::<(Arc<CacheKey>, CachedPlan)>().saturating_add(1)))
                    .saturating_add(32);
                let retained_entry_bytes = cache.entries.values().map(|plan| plan.accounted_bytes).sum::<usize>();
                let independent_total = expected_table_bytes.saturating_add(retained_entry_bytes);
                assert!(cache.total_bytes >= independent_total, "cache accounting must include the retained table high-water allocation");
            }
        }

        assert!(saw_growth, "sustained LRU churn must exercise HashMap table growth");
        assert!(cache.entries.len() <= MAX_CACHE_ENTRIES);
        assert!(cache.total_bytes <= MAX_CACHE_BYTES);

        // Reuse the grown table for byte-cap eviction. The independent retained-table allowance
        // must survive clearing entries, and true LRU behavior must remain intact.
        cache.clear_context(1);
        assert!(cache.entries.is_empty());
        let expected_bucket_count = high_water_capacity.saturating_mul(8).saturating_add(6) / 7;
        let expected_table_bytes = std::mem::size_of::<LayoutCache>()
            .saturating_add(expected_bucket_count.saturating_mul(std::mem::size_of::<(Arc<CacheKey>, CachedPlan)>().saturating_add(1)))
            .saturating_add(32);
        assert!(cache.total_bytes >= expected_table_bytes, "clearing Context entries must retain the grown table accounting");

        let large_key_for = |index: usize| {
            let text_length = MAX_CACHE_BYTES / 3;
            let mut text = String::with_capacity(text_length.saturating_add(16));
            text.extend(std::iter::repeat_n('x', text_length));
            text.push('-');
            text.push_str(&index.to_string());
            Arc::new(CacheKey {
                text,
                family_name: "family".to_string(),
                context_id: 1,
                font_size_bits: 12.0f32.to_bits(),
                avail_width_bits: 80.0f32.to_bits(),
                pixels_per_point_bits: 1.0f32.to_bits(),
                font_gen: 1,
                zoom_pct: 100,
                halign: 0,
            })
        };
        let least_recent = large_key_for(0);
        let touched = large_key_for(1);
        let newest = large_key_for(2);
        let empty_plan = || CachedPlan { lines: Arc::from([]), key_hash: 0, fallback_reason: None, accounted_bytes: 0, last_used: 0 };
        cache.insert(Arc::clone(&least_recent), empty_plan());
        cache.insert(Arc::clone(&touched), empty_plan());
        assert_eq!(cache.entries.len(), 2, "two large entries should fit before byte-cap pressure");
        assert!(cache.get(touched.as_ref()).is_some());
        cache.insert(Arc::clone(&newest), empty_plan());
        assert!(!cache.entries.contains_key(least_recent.as_ref()), "byte pressure must evict the least-recently-used entry");
        assert!(cache.entries.contains_key(touched.as_ref()));
        assert!(cache.entries.contains_key(newest.as_ref()));
        assert!(cache.total_bytes <= MAX_CACHE_BYTES);

        // The entry-count cap remains enforced after the table has grown and its entries were
        // cleared. This complements the byte-pressure LRU assertion above.
        cache.clear_context(1);
        for index in 0..=MAX_CACHE_ENTRIES {
            let key = Arc::new(CacheKey {
                text: format!("post-churn-entry-{index}"),
                family_name: "family".to_string(),
                context_id: 1,
                font_size_bits: 12.0f32.to_bits(),
                avail_width_bits: 80.0f32.to_bits(),
                pixels_per_point_bits: 1.0f32.to_bits(),
                font_gen: 1,
                zoom_pct: 100,
                halign: 0,
            });
            cache.insert(key, empty_plan());
            assert!(cache.entries.len() <= MAX_CACHE_ENTRIES);
            assert!(cache.total_bytes <= MAX_CACHE_BYTES);
        }
        assert_eq!(cache.entries.len(), MAX_CACHE_ENTRIES);
        assert!(!cache.entries.keys().any(|key| key.text == "post-churn-entry-0"));
        assert!(cache.entries.keys().any(|key| key.text == format!("post-churn-entry-{MAX_CACHE_ENTRIES}")));
    }

    #[test]
    fn cache_evicts_at_the_entry_limit() {
        let mut cache = LayoutCache::new();
        for index in 0..=MAX_CACHE_ENTRIES {
            let key = Arc::new(CacheKey {
                text: format!("entry-limit-{index}"),
                family_name: "family".to_string(),
                context_id: 1,
                font_size_bits: 12.0f32.to_bits(),
                avail_width_bits: 80.0f32.to_bits(),
                pixels_per_point_bits: 1.0f32.to_bits(),
                font_gen: 1,
                zoom_pct: 100,
                halign: 0,
            });
            cache.insert(Arc::clone(&key), CachedPlan { lines: Arc::from([]), key_hash: 0, fallback_reason: None, accounted_bytes: 0, last_used: 0 });
        }
        assert_eq!(cache.entries.len(), MAX_CACHE_ENTRIES);
        assert!(!cache.entries.keys().any(|key| key.text == "entry-limit-0"));
        assert!(cache.entries.keys().any(|key| key.text == format!("entry-limit-{MAX_CACHE_ENTRIES}")));
        assert!(cache.total_bytes <= MAX_CACHE_BYTES);
    }
}
