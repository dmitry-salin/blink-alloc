//! This module provides single-threaded blink allocator.

use core::{
    alloc::Layout,
    cell::Cell,
    hint::unreachable_unchecked,
    mem::{align_of, size_of},
    ptr::{self, NonNull},
};

use crate::{
    align_up::{align_ptr_offset, align_up},
    api::{AllocError, Allocator, BlinkAllocator, Global},
    cold,
};

/// 4 KB. Initial chunk size.
const CHUNK_START_SIZE: usize = 4096;

/// 32 MB. After this size, new chunk size is not aligned to next power of two.
const CHUNK_POWER_OF_TWO_THRESHOLD: usize = 1 << 25;

/// 4 KB. Alignment for chunks larger than `CHUNK_POWER_OF_TWO_THRESHOLD`.
const CHUNK_MIN_ALIGN: usize = 4096;

#[repr(C)]
#[repr(align(16))]
struct Chunk {
    used: Cell<usize>,
    cap: usize,
    prev: Option<NonNull<Chunk>>,
}

impl Chunk {
    fn _alloc(&self, base: *mut u8, layout: Layout) -> Result<NonNull<u8>, usize> {
        // Safety: `chunk.used` may not be larger than `chunk.cap`.
        // And `chunk.cap` is number of bytes allocated starting from `base`.
        let ptr = unsafe { base.add(self.used.get()) };

        let offset = align_ptr_offset(base, layout.align());
        if offset <= self.cap - self.used.get() {
            if layout.size() <= self.cap - self.used.get() - offset {
                // Safety: offset is within the chunk.
                let ptr = unsafe { ptr.add(offset) };
                self.used.set(self.used.get() + offset + layout.size());

                // Safety: `ptr` is within allocation after header.
                let ptr = unsafe { NonNull::new_unchecked(ptr) };
                return Ok(ptr);
            }
        }

        Err(self
            .used
            .get()
            .saturating_add(offset)
            .saturating_add(layout.size()))
    }

    // Safety: `chunk` must be a pointer to the chunk allocation.
    #[inline(always)]
    unsafe fn alloc(chunk: NonNull<Self>, layout: Layout) -> Result<NonNull<u8>, usize> {
        // Safety: `chunk` is a valid pointer to `Chunk`.
        // Thus it is safe to make a pointer next to it.
        let base = unsafe { chunk.as_ptr().add(1).cast::<u8>() };
        chunk.as_ref()._alloc(base, layout)
    }
}

/// Single-threaded blink allocator.
pub struct BlinkAlloc<A: Allocator> {
    root: Cell<Option<NonNull<Chunk>>>,
    prev_chunks_size: Cell<usize>,
    allocator: A,
}

impl<A> Default for BlinkAlloc<A>
where
    A: Allocator + Default,
{
    #[inline(always)]
    fn default() -> Self {
        Self::new_in(Default::default())
    }
}

#[cfg(feature = "alloc")]
impl BlinkAlloc<Global> {
    /// Creates new blink allocator that uses global allocator
    /// to allocate memory chunks.
    ///
    /// See [`BlinkAlloc::new_in`] for using custom allocator.
    #[inline(always)]
    pub fn new() -> Self {
        BlinkAlloc::new_in(Global)
    }
}

impl<A> BlinkAlloc<A>
where
    A: Allocator,
{
    /// Creates new blink allocator that uses provided allocator
    /// to allocate memory chunks.
    ///
    /// See [`BlinkAlloc::new`] for using global allocator.
    #[inline(always)]
    pub fn new_in(allocator: A) -> Self {
        BlinkAlloc {
            root: Cell::new(None),
            prev_chunks_size: Cell::new(0),
            allocator,
        }
    }

    /// Main allocation method.
    /// All different allocation methods are implemented in terms of this one.
    fn _alloc(&self, layout: Layout) -> Result<NonNull<u8>, AllocError> {
        let Some(mut min_chunk_size) = layout.size().checked_add(layout.align()).and_then(|l| l.checked_add(size_of::<Chunk>())) else {
            // Layout is too large to fit into a chunk
            return Err(AllocError);
        };

        let chunk = self.root.get();

        if let Some(chunk) = chunk {
            // Safety: `chunk` is a valid pointer to chunk allocation.
            let res = unsafe { Chunk::alloc(chunk, layout) };
            match res {
                Ok(ptr) => return Ok(ptr),
                Err(min_size) => {
                    min_chunk_size = min_chunk_size.max(min_size);
                }
            }
        }

        // Have to allocate new chunk.
        cold();

        let mut chunk_size = self.prev_chunks_size.get().saturating_add(min_chunk_size);
        if chunk_size < CHUNK_POWER_OF_TWO_THRESHOLD {
            chunk_size = chunk_size.next_power_of_two();
        } else {
            chunk_size = align_up(chunk_size, CHUNK_MIN_ALIGN).unwrap_or(chunk_size);
        };
        chunk_size = chunk_size.max(CHUNK_START_SIZE);

        let Ok(chunk_layout) = Layout::from_size_align(chunk_size, align_of::<Chunk>()) else {
            // Failed to construct chunk layout.
            return Err(AllocError);
        };

        let chunk_ptr = self.allocator.allocate(chunk_layout)?;
        let new_chunk = chunk_ptr.cast::<Chunk>();

        // Safety: `chunk_ptr` is a valid pointer to chunk allocation.
        unsafe {
            ptr::write(
                new_chunk.as_ptr(),
                Chunk {
                    used: Cell::new(0),
                    cap: chunk_size - size_of::<Chunk>(),
                    prev: chunk,
                },
            );
        }

        // Safety: `chunk` is a valid pointer to chunk allocation.
        let res = unsafe { Chunk::alloc(new_chunk, layout) };
        let Ok(ptr) = res else {
            // Safety: chunk size must fit requested allocation.
            unsafe { unreachable_unchecked() }
        };

        if let Some(chunk) = chunk {
            let last_chunk_cap = unsafe { chunk.as_ref().cap };
            self.prev_chunks_size
                .set(self.prev_chunks_size.get().saturating_add(last_chunk_cap));
        }

        self.root.set(Some(new_chunk));
        Ok(ptr)
    }

    /// Resets this allocator, deallocating all chunks except the last one.
    /// Last chunk will be reused.
    /// With steady memory usage after few iterations
    /// chunk size should be sufficient for all allocations between resets.
    pub fn reset(&mut self) {
        self._reset(true);
    }

    fn _reset(&mut self, keep_last: bool) {
        let mut next = if keep_last {
            let Some(mut chunk) = self.root.get() else {
                return;
            };

            // Safety: `chunk` is a valid pointer to chunk allocation.
            // This function owns mutable reference to `self`.
            unsafe {
                let chunk = chunk.as_mut();
                chunk.used.set(0);
                chunk.prev.take()
            }
        } else {
            self.root.take()
        };

        while let Some(chunk) = next {
            let chunk_layout = {
                // Safety: `chunk` is a valid pointer to chunk allocation.
                let chunk = unsafe { chunk.as_ref() };
                next = chunk.prev;

                let chunk_size = chunk.cap + size_of::<Chunk>();

                let Ok(chunk_layout) = Layout::from_size_align(chunk_size, align_of::<Chunk>()) else {
                    // Safety: chunk was allocated with this layout.
                    unsafe { unreachable_unchecked() }
                };

                chunk_layout
            };

            // Safety: `chunk` is a valid pointer to chunk allocation.
            // Allocated from this allocator with this layout.
            unsafe { self.allocator.deallocate(chunk.cast(), chunk_layout) }
        }
    }
}

impl<A> Drop for BlinkAlloc<A>
where
    A: Allocator,
{
    fn drop(&mut self) {
        self._reset(false);
    }
}

unsafe impl<A> Allocator for BlinkAlloc<A>
where
    A: Allocator,
{
    #[inline(always)]
    fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, AllocError> {
        self._alloc(layout)
    }

    #[inline(always)]
    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        let _ = (ptr, layout);
    }
}

unsafe impl<A> Allocator for &mut BlinkAlloc<A>
where
    A: Allocator,
{
    #[inline(always)]
    fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, AllocError> {
        self._alloc(layout)
    }

    #[inline(always)]
    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        let _ = (ptr, layout);
    }
}

unsafe impl<A> BlinkAllocator for BlinkAlloc<A>
where
    A: Allocator,
{
    #[inline(always)]
    unsafe fn reset(&mut self) {
        self._reset(true)
    }
}
