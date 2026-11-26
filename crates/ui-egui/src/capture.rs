//! One-shot completion slots for screenshot requests.
//!
//! Applications retain request IDs, output paths, deadlines, settling policy, geometry epochs and
//! aggregate admission budgets. This module only correlates one capture with one slot, checks the
//! retained image against a per-image byte bound, and wakes the owning viewport.
//!
//! [`CaptureSlot::new`] does not issue a screenshot command. Send the returned [`egui::ScreenshotCallback`]
//! with [`egui::ViewportCommand::Screenshot`] when the application decides the capture is due, or
//! complete it directly from a headless renderer. The callback holds only a weak reference to the
//! slot, so dropping or cancelling the request releases its image and makes late results inert.
//!
//! Every retry needs a fresh slot and a fresh callback from [`CaptureSlot::new`]; a callback is
//! one-shot, and a slot ignores completions after its result was taken, cancelled or dropped. The
//! slot does not track whether a request is still open: keep that pending/closed distinction in the
//! application's request record.
//!
//! A completion that publishes its result just before a concurrent [`CaptureSlot::cancel`] or drop
//! can still deliver its single wakeup afterwards. That extra repaint is harmless: if the
//! cancellation won, the image is released and a later [`CaptureSlot::take`] returns `None`; if
//! `take` won first, the caller already owns the result and nothing else changes. A completion that
//! observes a cancelled or taken slot stores no image and does not wake. Cancellation and completion
//! are therefore not atomic with respect to the wakeup, and this module never waits for another
//! thread while holding the slot lock.
//!
//! [`CaptureSlot`] holds a strong [`egui::Context`]. Keep it in the application's request record;
//! storing it in `Context`-owned data (`egui::Memory::data` or a registered plugin) forms a
//! `Context` → slot → `Context` cycle that keeps both alive.

use std::mem::size_of;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use egui::{Color32, ColorImage, Context, ScreenshotCallback, ViewportId};

/// Why a completed screenshot was rejected and released.
///
/// The enum is `#[non_exhaustive]`: consumers should match the known variants and use a wildcard
/// arm for a future rejection reason.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureError {
    /// The checked `width * height` pixel count, or its `width * height * size_of::<Color32>()`
    /// byte count, overflowed `usize`.
    DimensionsOverflow { width: usize, height: usize },
    /// The pixel buffer does not match the reported image dimensions, or the image has zero width
    /// or height.
    MalformedImage { width: usize, height: usize, pixels: usize },
    /// The checked image size exceeds the slot's per-image byte bound.
    ImageTooLarge { bytes: usize, max_bytes: usize },
}

impl core::fmt::Display for CaptureError {
    /// Format a human-readable rejection reason. The wording is informative only and is not an
    /// API contract; the variants keep checked-overflow, malformed-image and size-cap cases
    /// distinguishable.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::DimensionsOverflow { width, height } => {
                write!(f, "screenshot dimensions {width}×{height} overflow the checked pixel or byte count")
            }
            Self::MalformedImage { width, height, pixels } => write!(f, "screenshot has {pixels} pixels for its {width}×{height} dimensions"),
            Self::ImageTooLarge { bytes, max_bytes } => write!(f, "screenshot uses {bytes} bytes, over the {max_bytes}-byte per-image limit"),
        }
    }
}

impl std::error::Error for CaptureError {}

enum SlotState {
    Pending,
    Ready(Result<Arc<ColorImage>, CaptureError>),
    Taken,
    Cancelled,
}

struct Shared {
    state: Mutex<SlotState>,
    /// The live request owns the strong context; the callback only upgrades it briefly to wake.
    context: Context,
    viewport: ViewportId,
    max_image_bytes: usize,
}

/// The live request side of a capture. Keep it alive until the result is taken or the request is
/// cancelled; dropping it cancels the completion and releases any retained image.
///
/// This type is `Send + Sync`; the application may hold it in its request record and poll
/// [`Self::take`] from its logic pass. Do not store it in `Context`-owned storage: the slot holds a
/// strong [`egui::Context`], so `egui::Memory::data` or a registered plugin would create a cycle.
#[must_use = "the slot must be kept alive to receive and take the screenshot"]
pub struct CaptureSlot {
    shared: Arc<Shared>,
}

impl CaptureSlot {
    /// Create a completion slot and its one-shot callback.
    ///
    /// `max_image_bytes` bounds the single retained image only. The application must separately
    /// reserve its aggregate in-flight/completed budget before issuing the request.
    ///
    /// This function does not send a viewport command. If the request is issued in a UI pass, send
    /// the callback with [`egui::ViewportCommand::Screenshot`]; a pending screenshot command itself
    /// requests the paint needed while the window is hidden. Issue that command at most once per
    /// slot, after the application has settled the frame it wants; every retry needs a fresh slot
    /// and callback.
    pub fn new(context: &Context, viewport: ViewportId, max_image_bytes: usize) -> (Self, ScreenshotCallback) {
        let shared = Arc::new(Shared { state: Mutex::new(SlotState::Pending), context: context.clone(), viewport, max_image_bytes });
        let weak = Arc::downgrade(&shared);
        let callback = ScreenshotCallback::new(move |image| complete(&weak, image));
        (Self { shared }, callback)
    }

    /// Take the completed result, at most once.
    ///
    /// Returns `None` while the capture is pending, after it was cancelled or dropped, and after a
    /// previous call already took the image or error. The slot cannot distinguish those states;
    /// the application's request record owns the pending/closed distinction. Invalid images are
    /// returned as an error so the application can report completion failure with its own request
    /// identity.
    pub fn take(&self) -> Option<Result<Arc<ColorImage>, CaptureError>> {
        let mut state = lock(&self.shared.state);
        if !matches!(&*state, SlotState::Ready(_)) {
            return None;
        }
        match core::mem::replace(&mut *state, SlotState::Taken) {
            SlotState::Ready(result) => Some(result),
            _ => None,
        }
    }

    /// Cancel the request and release any completed image.
    ///
    /// A completion that published its result before this call may still deliver one harmless
    /// repaint afterwards; the slot retains no image and a later [`Self::take`] returns `None`. A
    /// completion that observes the cancellation stores no image and does not wake. Calling `cancel`
    /// after [`Self::take`] is harmless. Any retained image is dropped after the slot mutex is
    /// unlocked.
    pub fn cancel(&self) {
        close(&self.shared);
    }

    #[cfg(test)]
    /// Nonblocking probe used by the wakeup test to prove that the slot mutex is free while the
    /// repaint observer runs.
    fn lock_is_free(&self) -> bool {
        self.shared.state.try_lock().is_ok()
    }

    #[cfg(test)]
    /// Count strong references to the shared slot state, proving callbacks retain none.
    fn state_strong_count(&self) -> usize {
        Arc::strong_count(&self.shared)
    }

    #[cfg(test)]
    /// A weak handle to the shared state, used to inject a second completion directly when egui has
    /// already consumed the public callback.
    fn state_weak(&self) -> Weak<Shared> {
        Arc::downgrade(&self.shared)
    }
}

impl Drop for CaptureSlot {
    /// Cancel the request as the slot goes away. A later completion upgrades no live state and
    /// releases its image, a retained ready image is dropped, and no repaint is requested. Results
    /// already taken by the caller are unaffected.
    fn drop(&mut self) {
        close(&self.shared);
    }
}

impl core::fmt::Debug for CaptureSlot {
    /// Format only the viewport and omit the remaining fields. The retained image and error payload
    /// are deliberately hidden so logging cannot hold or dump screenshot contents.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CaptureSlot").field("viewport", &self.shared.viewport).finish_non_exhaustive()
    }
}

/// Lock the slot state, recovering from poisoning instead of panicking. The guard must be
/// released before entering the egui context or a repaint observer; completion and close do so
/// before waking the viewport.
fn lock(state: &Mutex<SlotState>) -> MutexGuard<'_, SlotState> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Mark the slot closed and drop any retained image after releasing the mutex.
fn close(shared: &Shared) {
    let previous = {
        let mut state = lock(&shared.state);
        core::mem::replace(&mut *state, SlotState::Cancelled)
    };
    drop(previous);
}

/// Validate a completed image, store it, and wake the owning viewport.
///
/// The slot lock is released before calling into the context, so a repaint observer may call
/// [`CaptureSlot::take`] or [`CaptureSlot::cancel`] re-entrantly. Inside an observer, only consume or
/// release that value; any further `Context` access must happen outside the observer.
/// [`egui::Context::request_repaint_of`] takes egui's context lock. Ordinary `Ui` closures and egui
/// 0.37 plugin hooks run outside that lock, as do renderer/worker callbacks. Repaint observers and
/// request-repaint callbacks run under it: completing from either would deadlock when requesting
/// the repaint. This function never waits under the slot lock.
fn complete(weak: &Weak<Shared>, image: Arc<ColorImage>) {
    let Some(shared) = weak.upgrade() else {
        // The request was dropped; the image is released here without a reply.
        return;
    };
    let result = checked_image(image, shared.max_image_bytes);
    let should_wake = {
        let mut state = lock(&shared.state);
        if matches!(&*state, SlotState::Pending) {
            *state = SlotState::Ready(result);
            true
        } else {
            // Cancelled, already taken, or (defensively) completed twice: drop the image.
            false
        }
    };
    // The slot lock is released before calling into the context, so a repaint observer may take
    // the result or cancel the request re-entrantly without deadlocking.
    if should_wake {
        shared.context.request_repaint_of(shared.viewport);
    }
}

/// Validate `image` against the slot's per-image byte bound before publishing it.
///
/// Zero width or height is malformed before any arithmetic; the pixel count and its byte size use
/// checked multiplication; the buffer length must equal `width * height`; and only then is the
/// byte count compared with `max_image_bytes`, rejecting a count greater than it (equality is
/// accepted). `max_image_bytes` bounds this one image, not an aggregate request budget. Returns the
/// same `Arc` without waking the viewport or requesting a repaint.
fn checked_image(image: Arc<ColorImage>, max_image_bytes: usize) -> Result<Arc<ColorImage>, CaptureError> {
    let [width, height] = image.size;
    if width == 0 || height == 0 {
        return Err(CaptureError::MalformedImage { width, height, pixels: image.pixels.len() });
    }
    let pixels = width.checked_mul(height).ok_or(CaptureError::DimensionsOverflow { width, height })?;
    let bytes = pixels.checked_mul(size_of::<Color32>()).ok_or(CaptureError::DimensionsOverflow { width, height })?;
    if image.pixels.len() != pixels {
        return Err(CaptureError::MalformedImage { width, height, pixels: image.pixels.len() });
    }
    if bytes > max_image_bytes {
        return Err(CaptureError::ImageTooLarge { bytes, max_bytes: max_image_bytes });
    }
    Ok(image)
}

#[cfg(test)]
mod tests;
