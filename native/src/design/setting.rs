//! `Setting`: a runtime design value (theme, scale, typography) that is process-wide in the app and per-thread under `cargo test`.
//!
//! The app reads these from every view and from the editor's layout, so they
//! are globals. Tests run in parallel threads, and some switch the theme or
//! the body size; with one global, a renderer test could lay out at a size
//! another test set mid-way. `iced_test` drives `update` and `view` on the
//! test's own thread, so giving each test thread its own copy isolates them
//! without changing how the app behaves.

#[cfg(not(test))]
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) struct Setting {
    #[cfg(not(test))]
    value: AtomicU64,
    #[cfg_attr(not(test), allow(dead_code))]
    initial: u64,
}

#[cfg(test)]
thread_local! {
    /// Values set on this thread, keyed by the `Setting`'s address.
    static VALUES: std::cell::RefCell<std::collections::HashMap<usize, u64>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

impl Setting {
    pub(crate) const fn new(initial: u64) -> Self {
        Self {
            #[cfg(not(test))]
            value: AtomicU64::new(initial),
            initial,
        }
    }

    #[cfg(not(test))]
    pub(crate) fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }

    /// Store `value`, returning the previous one.
    #[cfg(not(test))]
    pub(crate) fn replace(&self, value: u64) -> u64 {
        self.value.swap(value, Ordering::Relaxed)
    }

    #[cfg(test)]
    fn key(&self) -> usize {
        std::ptr::from_ref(self) as usize
    }

    #[cfg(test)]
    pub(crate) fn get(&self) -> u64 {
        VALUES.with(|values| {
            values
                .borrow()
                .get(&self.key())
                .copied()
                .unwrap_or(self.initial)
        })
    }

    #[cfg(test)]
    pub(crate) fn replace(&self, value: u64) -> u64 {
        let previous = self.get();
        VALUES.with(|values| values.borrow_mut().insert(self.key(), value));
        previous
    }
}

#[cfg(test)]
mod tests {
    use super::Setting;

    static SAMPLE: Setting = Setting::new(7);

    #[test]
    fn each_test_thread_sees_its_own_value() {
        assert_eq!(SAMPLE.replace(9), 7);
        let other = std::thread::spawn(|| SAMPLE.get()).join().unwrap();
        assert_eq!(other, 7, "another thread keeps the initial value");
        assert_eq!(SAMPLE.get(), 9);
    }
}
