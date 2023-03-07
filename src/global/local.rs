use core::{
    alloc::{GlobalAlloc, Layout},
    ptr::{null_mut, NonNull},
};

use allocator_api2::alloc::Allocator;

use crate::local::BlinkAlloc;

switch_std_default! {
    /// [`GlobalAlloc`] implementation based on [`BlinkAlloc`].
    pub struct UnsafeGlobalBlinkAlloc<A: Allocator = +std::alloc::System> {
        inner: BlinkAlloc<A>,
    }
}

// The user is responsible for ensuring that this allocator
// won't be used concurrently.
// To make this sound, `UnsafeGlobalBlinkAlloc::new` and `UnsafeGlobalBlinkAlloc::new_in`
// are marked unsafe.
unsafe impl<A: Allocator + Sync> Sync for UnsafeGlobalBlinkAlloc<A> {}

#[cfg(feature = "std")]
impl UnsafeGlobalBlinkAlloc<std::alloc::System> {
    /// Create a new [`UnsafeGlobalBlinkAlloc`].
    ///
    /// Const function can be used to initialize a static variable.
    ///
    /// # Safety
    ///
    /// This method is unsafe because this type is not thread-safe
    /// but implements `Sync`.
    /// Allocator returned by this method must not be used concurrently.
    ///
    /// For safer alternative see [`GlobalBlinkAlloc`](https://docs.rs/blink-alloc/0.2.2/blink_alloc/struct.GlobalBlinkAlloc.html).
    ///
    /// # Example
    ///
    /// ```
    /// use blink_alloc::UnsafeGlobalBlinkAlloc;
    ///
    /// // Safety: This program is single-threaded.
    /// #[global_allocator]
    /// static GLOBAL_ALLOC: UnsafeGlobalBlinkAlloc = unsafe { UnsafeGlobalBlinkAlloc::new() };
    ///
    /// let _ = Box::new(42);
    /// let _ = vec![1, 2, 3];
    /// ```
    pub const unsafe fn new() -> Self {
        Self {
            inner: BlinkAlloc::new_in(std::alloc::System),
        }
    }

    /// Create a new [`UnsafeGlobalBlinkAlloc`].
    /// With this method you can specify initial chunk size.
    ///
    /// Const function can be used to initialize a static variable.
    ///
    /// # Safety
    ///
    /// This method is unsafe because this type is not thread-safe
    /// but implements `Sync`.
    /// Allocator returned by this method must not be used concurrently.
    ///
    /// For safer alternative see [`GlobalBlinkAlloc`](https://docs.rs/blink-alloc/0.2.2/blink_alloc/struct.GlobalBlinkAlloc.html).
    ///
    /// # Example
    ///
    /// ```
    /// use blink_alloc::UnsafeGlobalBlinkAlloc;
    ///
    /// // Safety: This program is single-threaded.
    /// #[global_allocator]
    /// static GLOBAL_ALLOC: UnsafeGlobalBlinkAlloc = unsafe { UnsafeGlobalBlinkAlloc::with_chunk_size(1024) };
    ///
    /// let _ = Box::new(42);
    /// let _ = vec![1, 2, 3];
    /// ```
    pub const unsafe fn with_chunk_size(chunk_size: usize) -> Self {
        Self {
            inner: BlinkAlloc::with_chunk_size_in(chunk_size, std::alloc::System),
        }
    }
}

impl<A> UnsafeGlobalBlinkAlloc<A>
where
    A: Allocator,
{
    /// Create a new [`UnsafeGlobalBlinkAlloc`]
    /// with specified underlying allocator.
    ///
    /// Const function can be used to initialize a static variable.
    ///
    /// # Safety
    ///
    /// This method is unsafe because this type is not thread-safe
    /// but implements `Sync`.
    /// Allocator returned by this method must not be used concurrently.
    ///
    /// For safer alternative see [`GlobalBlinkAlloc`](https://docs.rs/blink-alloc/0.2.2/blink_alloc/struct.GlobalBlinkAlloc.html).
    ///
    /// # Example
    ///
    /// ```
    /// # #[cfg(feature = "std")] fn main() {
    /// use blink_alloc::UnsafeGlobalBlinkAlloc;
    ///
    /// // Safety: This program is single-threaded.
    /// #[global_allocator]
    /// static GLOBAL_ALLOC: UnsafeGlobalBlinkAlloc<std::alloc::System> = unsafe { UnsafeGlobalBlinkAlloc::new_in(std::alloc::System) };
    ///
    /// let _ = Box::new(42);
    /// let _ = vec![1, 2, 3];
    /// # }
    /// # #[cfg(not(feature = "std"))] fn main() {}
    /// ```
    pub const unsafe fn new_in(allocator: A) -> Self {
        Self {
            inner: BlinkAlloc::new_in(allocator),
        }
    }

    /// Create a new [`UnsafeGlobalBlinkAlloc`]
    /// with specified underlying allocator.
    /// With this method you can specify initial chunk size.
    ///
    /// Const function can be used to initialize a static variable.
    ///
    /// # Safety
    ///
    /// This method is unsafe because this type is not thread-safe
    /// but implements `Sync`.
    /// Allocator returned by this method must not be used concurrently.
    ///
    /// For safer alternative see [`GlobalBlinkAlloc`](https://docs.rs/blink-alloc/0.2.2/blink_alloc/struct.GlobalBlinkAlloc.html).
    ///
    /// # Example
    ///
    /// ```
    /// # #[cfg(feature = "std")] fn main() {
    /// use blink_alloc::UnsafeGlobalBlinkAlloc;
    ///
    /// // Safety: This program is single-threaded.
    /// #[global_allocator]
    /// static GLOBAL_ALLOC: UnsafeGlobalBlinkAlloc<std::alloc::System> = unsafe { UnsafeGlobalBlinkAlloc::with_chunk_size_in(1024, std::alloc::System) };
    ///
    /// let _ = Box::new(42);
    /// let _ = vec![1, 2, 3];
    /// # }
    /// # #[cfg(not(feature = "std"))] fn main() {}
    /// ```
    pub const unsafe fn with_chunk_size_in(chunk_size: usize, allocator: A) -> Self {
        Self {
            inner: BlinkAlloc::with_chunk_size_in(chunk_size, allocator),
        }
    }

    /// Resets this allocator, deallocating all chunks except the last one.
    /// Last chunk will be reused.
    /// With steady memory usage after few iterations
    /// one chunk should be sufficient for all allocations between resets.
    ///
    /// # Safety
    ///
    /// Memory allocated from this allocator becomes invalidated.
    /// The user is responsible to ensure that previously allocated memory
    /// won't be used after reset.
    ///
    /// # Example
    ///
    /// ```
    /// # #[cfg(feature = "std")] fn main() {
    /// use blink_alloc::UnsafeGlobalBlinkAlloc;
    ///
    /// #[global_allocator]
    /// static GLOBAL_ALLOC: UnsafeGlobalBlinkAlloc = unsafe { UnsafeGlobalBlinkAlloc::new() };
    ///
    /// let b = Box::new(42);
    /// let v = vec![1, 2, 3];
    /// drop(b);
    /// drop(v);
    ///
    /// // Safety: allocated memory won't be used after reset.
    /// unsafe { GLOBAL_ALLOC.reset() };
    /// # }
    /// # #[cfg(not(feature = "std"))] fn main() {}
    /// ```
    #[inline(always)]
    pub unsafe fn reset(&self) {
        self.inner.reset_unchecked();
    }
}

unsafe impl<A> GlobalAlloc for UnsafeGlobalBlinkAlloc<A>
where
    A: Allocator,
{
    #[inline]
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        match self.inner.allocate(layout) {
            Ok(ptr) => ptr.as_ptr().cast(),
            Err(_) => null_mut(),
        }
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        if let Some(ptr) = NonNull::new(ptr) {
            self.inner.deallocate(ptr, layout.size());
        }
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: core::alloc::Layout) -> *mut u8 {
        match self.inner.allocate_zeroed(layout) {
            Ok(ptr) => ptr.as_ptr().cast(),
            Err(_) => null_mut(),
        }
    }

    #[inline]
    unsafe fn realloc(
        &self,
        ptr: *mut u8,
        layout: core::alloc::Layout,
        new_size: usize,
    ) -> *mut u8 {
        let Ok(new_layout) = Layout::from_size_align(new_size, layout.align()) else {
            return null_mut();
        };

        let result = match NonNull::new(ptr) {
            None => self.inner.allocate(new_layout),
            Some(ptr) => self.inner.resize(ptr, layout, new_layout),
        };

        match result {
            Ok(ptr) => ptr.as_ptr().cast(),
            Err(_) => null_mut(),
        }
    }
}
