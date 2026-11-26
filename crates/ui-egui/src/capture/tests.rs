use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};

use super::*;

/// Allocate a filled `Arc<ColorImage>` fixture of `size` with every pixel set to `color`.
fn image(size: [usize; 2], color: egui::Color32) -> Arc<ColorImage> {
    Arc::new(ColorImage::filled(size, color))
}

/// Install a repaint observer and return the viewport requests it records.
fn observe_repaints(ctx: &Context) -> Arc<Mutex<Vec<ViewportId>>> {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed = requests.clone();
    ctx.set_repaint_observer(move |info| observed.lock().unwrap().push(info.viewport_id));
    requests
}

#[test]
/// Compile-time assertion that `CaptureSlot` and `ScreenshotCallback` are `Send + Sync`.
fn slot_is_send_and_sync() {
    /// Compile-time helper requiring `T: Send + Sync`; the enclosing test uses it to
    /// assert the public capture types are thread-safe.
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<CaptureSlot>();
    assert_send_sync::<ScreenshotCallback>();
}

#[test]
/// Two independent slots for different viewports may complete in reverse order; each `take`
/// returns exactly its own image and dimensions.
fn distinct_requests_complete_in_reverse_order() {
    let ctx = Context::default();
    let first_viewport = ViewportId::from_hash_of("first-window");
    let second_viewport = ViewportId::from_hash_of("second-window");
    let (first, first_callback) = CaptureSlot::new(&ctx, first_viewport, 1024);
    let (second, second_callback) = CaptureSlot::new(&ctx, second_viewport, 1024);
    let first_image = image([2, 1], egui::Color32::RED);
    let second_image = image([1, 3], egui::Color32::BLUE);

    // The renderer may answer the later request first.
    second_callback.complete(second_image.clone());
    first_callback.complete(first_image.clone());

    let taken_first = first.take().unwrap().unwrap();
    let taken_second = second.take().unwrap().unwrap();
    assert!(Arc::ptr_eq(&taken_first, &first_image));
    assert!(Arc::ptr_eq(&taken_second, &second_image));
    assert_eq!(taken_first.size, [2, 1]);
    assert_eq!(taken_second.size, [1, 3]);
}

#[test]
/// Slots keep separate results and byte caps: a 2×2 image over the first slot's 4-byte cap is
/// rejected as too large while the second stays pending, then the second accepts its own image and
/// a taken slot remains empty.
fn slots_do_not_share_results_or_byte_bounds() {
    let ctx = Context::default();
    let first_viewport = ViewportId::from_hash_of("first-slot");
    let second_viewport = ViewportId::from_hash_of("second-slot");
    let (first, first_callback) = CaptureSlot::new(&ctx, first_viewport, 4);
    let (second, second_callback) = CaptureSlot::new(&ctx, second_viewport, 16);

    let rejected_for_first = image([2, 2], egui::Color32::RED);
    first_callback.complete(rejected_for_first);

    assert!(second.take().is_none(), "completing one slot leaves the other pending");
    assert!(matches!(first.take(), Some(Err(CaptureError::ImageTooLarge { bytes: 16, max_bytes: 4 }))));

    let accepted_for_second = image([2, 2], egui::Color32::BLUE);
    second_callback.complete(accepted_for_second.clone());
    assert!(Arc::ptr_eq(&second.take().unwrap().unwrap(), &accepted_for_second));
    assert!(first.take().is_none(), "a taken slot stays empty");
}

#[test]
/// Cloned callbacks share one completion: the first result wins, the losing image is released, and
/// `take` yields the result only once.
fn callback_clones_share_one_completion_and_take_is_single_use() {
    let ctx = Context::default();
    let (slot, callback) = CaptureSlot::new(&ctx, ViewportId::ROOT, 1024);
    let duplicate = callback.clone();
    let first_image = image([1, 1], egui::Color32::RED);
    let second_image = image([1, 1], egui::Color32::BLUE);
    let weak_loser = Arc::downgrade(&second_image);

    callback.complete(first_image.clone());
    duplicate.complete(second_image);
    assert!(weak_loser.upgrade().is_none(), "the losing completion releases its image");

    let taken = slot.take().unwrap().unwrap();
    assert!(Arc::ptr_eq(&taken, &first_image), "the first completion wins");
    assert!(slot.take().is_none(), "the result is taken once");
}

#[test]
/// After `take`, both a duplicate callback and a direct completion release their images without
/// storing a result or requesting a repaint.
fn duplicate_completion_after_take_neither_stores_nor_wakes() {
    let ctx = Context::default();
    let requests = observe_repaints(&ctx);
    let (slot, callback) = CaptureSlot::new(&ctx, ViewportId::ROOT, 1024);
    let duplicate_callback = callback.clone();
    let first_image = image([1, 1], egui::Color32::RED);
    callback.complete(first_image.clone());
    assert!(Arc::ptr_eq(&slot.take().unwrap().unwrap(), &first_image));
    assert_eq!(requests.lock().unwrap().len(), 1, "the first completion wakes");

    let duplicate_image = image([1, 1], egui::Color32::BLUE);
    let weak_duplicate = Arc::downgrade(&duplicate_image);
    duplicate_callback.complete(duplicate_image);

    assert!(weak_duplicate.upgrade().is_none(), "a duplicate after take is released");
    assert!(slot.take().is_none());
    assert_eq!(requests.lock().unwrap().len(), 1, "a duplicate after take does not wake");

    // Deliver a second completion straight to the slot; egui already consumed its callback.
    let direct_image = image([1, 1], egui::Color32::GREEN);
    let weak_direct = Arc::downgrade(&direct_image);
    complete(&slot.state_weak(), direct_image);
    assert!(weak_direct.upgrade().is_none(), "a direct second completion after take is released");
    assert!(slot.take().is_none());
    assert_eq!(requests.lock().unwrap().len(), 1, "a direct second completion after take does not wake");
}

#[test]
/// An early `take` returns `None` without closing the request; a later completion still delivers.
fn take_before_completion_is_empty_and_keeps_the_request_open() {
    let ctx = Context::default();
    let (slot, callback) = CaptureSlot::new(&ctx, ViewportId::ROOT, 1024);
    assert!(slot.take().is_none());
    let expected = image([1, 1], egui::Color32::RED);
    callback.complete(expected.clone());
    assert!(Arc::ptr_eq(&slot.take().unwrap().unwrap(), &expected));
}

#[test]
/// Cancelling before completion releases a late image, leaves `take` empty, and suppresses the
/// repaint wakeup.
fn cancel_releases_late_images_without_waking() {
    let ctx = Context::default();
    let requests = observe_repaints(&ctx);
    let viewport = ViewportId::from_hash_of("cancelled-window");
    let (slot, callback) = CaptureSlot::new(&ctx, viewport, 1024);
    slot.cancel();

    let late_image = image([2, 2], egui::Color32::RED);
    let weak_image = Arc::downgrade(&late_image);
    callback.complete(late_image.clone());
    drop(late_image);

    assert!(weak_image.upgrade().is_none(), "a cancelled slot releases the late image");
    assert!(slot.take().is_none());
    assert!(requests.lock().unwrap().is_empty(), "cancellation suppresses the wakeup");
}

#[test]
/// A ready slot retains its image and has woken once; `cancel` releases the image and does not wake
/// again.
fn cancel_after_ready_releases_the_image() {
    let ctx = Context::default();
    let requests = observe_repaints(&ctx);
    let (slot, callback) = CaptureSlot::new(&ctx, ViewportId::ROOT, 1024);
    let completed = image([2, 2], egui::Color32::RED);
    let weak_image = Arc::downgrade(&completed);
    callback.complete(completed.clone());
    drop(completed);
    assert!(weak_image.upgrade().is_some(), "the ready slot retains the image");
    assert_eq!(requests.lock().unwrap().len(), 1, "the completion woke the viewport");

    slot.cancel();

    assert!(weak_image.upgrade().is_none(), "cancel releases a ready image");
    assert!(slot.take().is_none());
    assert_eq!(requests.lock().unwrap().len(), 1, "cancellation itself does not wake");
}

#[test]
/// Dropping the slot releases an image that is only retained by the completed slot.
fn dropping_the_slot_releases_a_completed_image() {
    let ctx = Context::default();
    let (slot, callback) = CaptureSlot::new(&ctx, ViewportId::ROOT, 1024);
    let completed = image([2, 2], egui::Color32::RED);
    callback.complete(completed.clone());
    let weak_image = Arc::downgrade(&completed);
    drop(completed);
    assert!(weak_image.upgrade().is_some(), "the slot retains the completed image");

    drop(slot);
    assert!(weak_image.upgrade().is_none(), "dropping the slot releases its image");
}

#[test]
/// A completion after the slot is dropped releases the image and does not wake the viewport.
fn late_completion_after_drop_is_inert() {
    let ctx = Context::default();
    let requests = observe_repaints(&ctx);
    let viewport = ViewportId::ROOT;
    let (slot, callback) = CaptureSlot::new(&ctx, viewport, 1024);
    drop(slot);

    let late_image = image([1, 1], egui::Color32::RED);
    let weak_image = Arc::downgrade(&late_image);
    callback.complete(late_image.clone());
    drop(late_image);

    assert!(weak_image.upgrade().is_none());
    assert!(requests.lock().unwrap().is_empty(), "a late result does not wake the viewport");
}

#[test]
/// Callbacks hold only a weak reference: the slot state has one strong owner, dropping the slot
/// releases the context and state, and a later completion is inert.
fn callback_retains_only_a_weak_reference_to_live_state() {
    let ctx = Context::default();
    let requests = observe_repaints(&ctx);
    let (slot, callback) = CaptureSlot::new(&ctx, ViewportId::ROOT, 1024);
    let callback_clone = callback.clone();
    assert_eq!(slot.state_strong_count(), 1, "the callback must not own the slot state");

    let weak_state = slot.state_weak();
    drop(slot);
    assert!(weak_state.upgrade().is_none(), "dropping the slot releases its context and state");

    let late_image = image([1, 1], egui::Color32::RED);
    let weak_image = Arc::downgrade(&late_image);
    callback_clone.complete(late_image.clone());
    drop(late_image);
    assert!(weak_image.upgrade().is_none());
    assert!(requests.lock().unwrap().is_empty());
}

#[test]
/// A completion distinguishes the per-image size cap, a malformed pixel buffer, and overflow of
/// the pixel or byte count; a returned error is taken at most once.
fn rejects_oversized_malformed_and_overflowing_images() {
    let ctx = Context::default();

    let (slot, callback) = CaptureSlot::new(&ctx, ViewportId::ROOT, 16);
    callback.complete(image([3, 3], egui::Color32::RED));
    assert!(matches!(slot.take(), Some(Err(CaptureError::ImageTooLarge { bytes: 36, max_bytes: 16 }))));
    assert!(slot.take().is_none(), "a completion error is reported once");

    let (slot, callback) = CaptureSlot::new(&ctx, ViewportId::ROOT, 1024);
    callback.complete(Arc::new(ColorImage { size: [2, 2], source_size: egui::Vec2::ZERO, pixels: vec![egui::Color32::RED; 3] }));
    assert!(matches!(slot.take(), Some(Err(CaptureError::MalformedImage { width: 2, height: 2, pixels: 3 }))));

    let (slot, callback) = CaptureSlot::new(&ctx, ViewportId::ROOT, 1024);
    callback.complete(Arc::new(ColorImage { size: [usize::MAX, 2], source_size: egui::Vec2::ZERO, pixels: Vec::new() }));
    assert!(matches!(slot.take(), Some(Err(CaptureError::DimensionsOverflow { width, height })) if width == usize::MAX && height == 2));

    // The pixel count fits but the byte count does not.
    let wide = usize::MAX / 2;
    let (slot, callback) = CaptureSlot::new(&ctx, ViewportId::ROOT, 1024);
    callback.complete(Arc::new(ColorImage { size: [wide, 1], source_size: egui::Vec2::ZERO, pixels: Vec::new() }));
    assert!(matches!(slot.take(), Some(Err(CaptureError::DimensionsOverflow { width, height })) if width == wide && height == 1));
}

#[test]
/// Oversized and malformed images are dropped immediately and each rejection still wakes the error
/// consumer exactly once before `take` reports it.
fn rejected_oversized_and_malformed_images_are_released_before_take() {
    let ctx = Context::default();
    let requests = observe_repaints(&ctx);

    let (slot, callback) = CaptureSlot::new(&ctx, ViewportId::ROOT, 16);
    let oversized = image([3, 3], egui::Color32::RED);
    let weak_oversized = Arc::downgrade(&oversized);
    callback.complete(oversized);
    assert!(weak_oversized.upgrade().is_none(), "an oversized image is released");
    assert_eq!(*requests.lock().unwrap(), [ViewportId::ROOT], "rejection wakes the error consumer");
    assert!(matches!(slot.take(), Some(Err(CaptureError::ImageTooLarge { bytes: 36, max_bytes: 16 }))));

    let (slot, callback) = CaptureSlot::new(&ctx, ViewportId::ROOT, 1024);
    let malformed = Arc::new(ColorImage { size: [2, 2], source_size: egui::Vec2::ZERO, pixels: vec![egui::Color32::RED; 3] });
    let weak_malformed = Arc::downgrade(&malformed);
    callback.complete(malformed);
    assert!(weak_malformed.upgrade().is_none(), "a malformed image is released");
    assert_eq!(*requests.lock().unwrap(), [ViewportId::ROOT, ViewportId::ROOT]);
    assert!(matches!(slot.take(), Some(Err(CaptureError::MalformedImage { width: 2, height: 2, pixels: 3 }))));
}

#[test]
/// Any zero width or height is rejected as malformed before overflow or size-cap decisions, even
/// for extreme extents such as `usize::MAX × 0`.
fn zero_area_images_are_rejected_even_with_extreme_extents() {
    let ctx = Context::default();

    for size in [[0, 4], [4, 0], [0, 0], [usize::MAX, 0], [0, usize::MAX]] {
        let (slot, callback) = CaptureSlot::new(&ctx, ViewportId::ROOT, 1024);
        callback.complete(Arc::new(ColorImage { size, source_size: egui::Vec2::ZERO, pixels: Vec::new() }));
        let rejected = slot.take();
        assert!(
            matches!(rejected, Some(Err(CaptureError::MalformedImage { width, height, pixels }))
                if width == size[0] && height == size[1] && pixels == 0),
            "zero-area image {size:?} must be rejected"
        );
    }
}

#[test]
/// A 2×2 image at exactly the 16-byte cap is accepted: the bound is inclusive.
fn accepts_an_image_exactly_at_the_byte_bound() {
    let ctx = Context::default();
    let (slot, callback) = CaptureSlot::new(&ctx, ViewportId::ROOT, 16);
    let exact = image([2, 2], egui::Color32::GREEN);
    callback.complete(exact.clone());
    assert!(Arc::ptr_eq(&slot.take().unwrap().unwrap(), &exact));
}

#[test]
/// A worker thread completes a child-window slot; every recorded wake belongs to that slot's
/// viewport and the expected image is delivered.
fn another_thread_completes_and_wakes_only_the_owning_viewport() {
    let ctx = Context::default();
    let requests = observe_repaints(&ctx);
    let viewport = ViewportId::from_hash_of("child-window");
    let (slot, callback) = CaptureSlot::new(&ctx, viewport, 1024);
    let expected = image([1, 1], egui::Color32::RED);
    let sent = expected.clone();

    let handle = std::thread::spawn(move || callback.complete(sent));
    handle.join().unwrap();

    let requested = requests.lock().unwrap();
    assert!(!requested.is_empty(), "completion wakes the owning viewport");
    assert!(requested.iter().all(|requested_viewport| *requested_viewport == viewport), "completion wakes only the owning viewport: {requested:?}");
    drop(requested);
    assert!(Arc::ptr_eq(&slot.take().unwrap().unwrap(), &expected));
}

#[test]
/// The repaint observer can `take` the image re-entrantly: `try_lock` succeeds during the wakeup,
/// proving the slot lock is released before `request_repaint_of`.
fn wakeup_runs_after_the_slot_lock_is_released() {
    let ctx = Context::default();
    let (slot, callback) = CaptureSlot::new(&ctx, ViewportId::ROOT, 1024);
    let slot = Arc::new(slot);
    let weak_slot = Arc::downgrade(&slot);
    let observed = Arc::new(AtomicBool::new(false));
    let lock_free = Arc::new(AtomicBool::new(true));
    let taken = Arc::new(AtomicBool::new(false));
    let observer_observed = observed.clone();
    let observer_lock_free = lock_free.clone();
    let observer_taken = taken.clone();
    ctx.set_repaint_observer(move |info| {
        if info.viewport_id == ViewportId::ROOT {
            observer_observed.store(true, Ordering::SeqCst);
            if let Some(slot) = weak_slot.upgrade() {
                if slot.lock_is_free() {
                    observer_taken.store(slot.take().is_some(), Ordering::SeqCst);
                } else {
                    observer_lock_free.store(false, Ordering::SeqCst);
                }
            }
        }
    });

    callback.complete(image([1, 1], egui::Color32::RED));

    assert!(observed.load(Ordering::SeqCst), "completion wakes the owning viewport");
    assert!(lock_free.load(Ordering::SeqCst), "the slot lock is released before the wakeup");
    assert!(taken.load(Ordering::SeqCst), "the observer can consume the image reentrantly");
    assert!(slot.take().is_none());
}

/// Complete, cancel and take all racing on one slot. Every interleaving must leave at most one
/// wake and at most one taken result, and every image must be released once the slot and result
/// are dropped.
#[test]
fn complete_cancel_and_take_race_without_retaining_or_double_waking() {
    const RACE_TRIALS: usize = 100;

    for trial in 0..RACE_TRIALS {
        let ctx = Context::default();
        let requests = observe_repaints(&ctx);
        let viewport = ViewportId::from_hash_of("race-window");
        let (slot, callback) = CaptureSlot::new(&ctx, viewport, 4096);
        let slot = Arc::new(slot);
        let completed = image([1, 1], egui::Color32::RED);
        let weak_image = Arc::downgrade(&completed);
        let barrier = Arc::new(Barrier::new(3));

        let taken = std::thread::scope(|scope| {
            let completion_barrier = barrier.clone();
            let completion_image = completed.clone();
            scope.spawn(move || {
                completion_barrier.wait();
                callback.complete(completion_image);
            });

            let cancel_barrier = barrier.clone();
            let cancel_slot = slot.clone();
            scope.spawn(move || {
                cancel_barrier.wait();
                cancel_slot.cancel();
            });

            barrier.wait();
            slot.take()
        });

        let wake_count = requests.lock().unwrap().len();
        assert!(wake_count <= 1, "trial {trial}: one completion wakes at most once, got {wake_count}");
        if taken.is_some() {
            assert_eq!(wake_count, 1, "trial {trial}: a published result wakes exactly once");
        }
        if let Some(result) = &taken {
            match result {
                Ok(taken_image) => assert!(Arc::ptr_eq(taken_image, &completed), "trial {trial}: only the completed image is returned"),
                Err(error) => panic!("trial {trial}: a valid image was rejected: {error:?}"),
            }
        }

        drop(taken);
        drop(slot);
        drop(completed);
        assert!(weak_image.upgrade().is_none(), "trial {trial}: the slot and any taken result release the image");
    }
}

#[test]
/// A zero byte budget rejects any nonempty image as too large and wakes once.
fn zero_budget_rejects_a_nonempty_image_and_wakes() {
    let ctx = Context::default();
    let requests = observe_repaints(&ctx);
    let (slot, callback) = CaptureSlot::new(&ctx, ViewportId::ROOT, 0);
    callback.complete(image([1, 1], egui::Color32::GREEN));
    assert!(matches!(slot.take(), Some(Err(CaptureError::ImageTooLarge { bytes: 4, max_bytes: 0 }))));
    assert_eq!(*requests.lock().unwrap(), [ViewportId::ROOT]);
}

#[test]
/// Two threads racing on `take` produce exactly one result, and a subsequent take is empty.
fn concurrent_takers_receive_the_result_exactly_once() {
    let ctx = Context::default();
    let (slot, callback) = CaptureSlot::new(&ctx, ViewportId::ROOT, 4);
    let expected = image([1, 1], egui::Color32::GREEN);
    callback.complete(expected.clone());
    let barrier = Barrier::new(3);
    let results = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            barrier.wait();
            slot.take()
        });
        let second = scope.spawn(|| {
            barrier.wait();
            slot.take()
        });
        barrier.wait();
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(results.iter().filter(|result| result.is_some()).count(), 1);
    for result in results.into_iter().flatten() {
        assert!(Arc::ptr_eq(&result.unwrap(), &expected));
    }
    assert!(slot.take().is_none());
}

#[test]
/// A renderer thread can complete a capture while a UI closure is blocked; the image is delivered
/// and the owning child viewport is woken exactly once.
fn renderer_completion_during_a_live_ui_pass_can_wake_and_deliver() {
    let ctx = Context::default();
    let requests = observe_repaints(&ctx);
    let viewport = ViewportId::from_hash_of("render-readback");
    let (slot, callback) = CaptureSlot::new(&ctx, viewport, 4);
    let expected = image([1, 1], egui::Color32::GREEN);
    let sent = expected.clone();
    let (start_tx, start_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        scope.spawn(move || {
            start_rx.recv().unwrap();
            callback.complete(sent);
            done_tx.send(()).unwrap();
        });
        let mut start_tx = Some(start_tx);
        let mut output = ctx.run_ui(Default::default(), |ui| {
            ui.label("A live frame while readback completes");
            if let Some(tx) = start_tx.take() {
                tx.send(()).unwrap();
                done_rx.recv_timeout(std::time::Duration::from_secs(3)).expect("readback must complete while the UI closure is active");
            }
        });
        // This synchronization test has no renderer to apply the atlas upload.
        output.textures_delta.clear();
    });
    assert!(Arc::ptr_eq(&slot.take().unwrap().unwrap(), &expected));
    assert_eq!(requests.lock().unwrap().iter().filter(|id| **id == viewport).count(), 1);
}
