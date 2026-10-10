//! Data parallelism for bulk work over many independent items (adjusting every formula after
//! a row insert, finding every formula's precedents): the items are split into contiguous
//! chunks, one per thread, and the results come back in order.

/// Items below this are handled on the calling thread: threads would cost more than they save.
const MIN_PARALLEL: usize = 4096;

/// `items.iter().filter_map(f)`, on every processor for large inputs. On wasm, for small inputs,
/// or if a thread can't start or fails, it runs on the calling thread (so `f` may run twice for
/// some items: it must not have side effects).
pub fn filter_map<T, R, F>(items: &[T], f: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> Option<R> + Sync,
{
    let threads = if cfg!(target_arch = "wasm32") { 1 } else { std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(64) };
    if threads <= 1 || items.len() < MIN_PARALLEL {
        return items.iter().filter_map(&f).collect();
    }
    let chunk = items.len().div_ceil(threads);
    let f = &f;
    let parts: Option<Vec<Vec<R>>> = std::thread::scope(|s| {
        let handles: Vec<_> = items
            .chunks(chunk)
            .map(|part| {
                std::thread::Builder::new().name("gridcraft-par".into()).spawn_scoped(s, move || part.iter().filter_map(f).collect::<Vec<R>>())
            })
            .collect();
        let mut parts = Vec::with_capacity(handles.len());
        let mut ok = true;
        for h in handles {
            match h.map(|h| h.join()) {
                Ok(Ok(part)) => parts.push(part),
                _ => ok = false,
            }
        }
        ok.then_some(parts)
    });
    match parts {
        Some(parts) => parts.into_iter().flatten().collect(),
        None => items.iter().filter_map(f).collect(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn keeps_order_and_filters() {
        let items: Vec<u32> = (0..20_000).collect();
        let out = super::filter_map(&items, |x| (x % 3 == 0).then_some(x * 2));
        let want: Vec<u32> = items.iter().filter(|x| *x % 3 == 0).map(|x| x * 2).collect();
        assert_eq!(out, want);
    }
}
