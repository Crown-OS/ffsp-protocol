use std::sync::atomic::{AtomicU64, Ordering};

use crossbeam_queue::ArrayQueue;
use tokio::sync::Semaphore;

use crate::io::ChunkBuffer;

/// A fixed set of chunk buffers, recycled for the life of a transfer.
///
/// The pool running dry is the only backpressure in the system: a lane that cannot obtain a buffer
/// simply stops pulling work, which bounds memory regardless of how many lanes are open or how
/// large the payload is.
#[derive(Debug)]
pub struct ChunkBufferPool {
    idle: ArrayQueue<ChunkBuffer>,
    /// Mirrors the queue's depth so a caller can wait for a buffer instead of polling for one.
    permits: Semaphore,
    starvations: AtomicU64,
    buffer_capacity: usize,
}

impl ChunkBufferPool {
    /// Allocates every buffer up front, so nothing on the data path ever allocates.
    pub fn new(count: usize, buffer_capacity: usize) -> Self {
        let count = count.max(1);
        let idle = ArrayQueue::new(count);
        for _ in 0..count {
            let _ = idle.push(ChunkBuffer::with_capacity(buffer_capacity));
        }
        Self {
            idle,
            permits: Semaphore::new(count),
            starvations: AtomicU64::new(0),
            buffer_capacity,
        }
    }

    /// Takes a cleared buffer, or `None` when the pool is empty and the caller should wait.
    pub fn take(&self) -> Option<ChunkBuffer> {
        let Ok(permit) = self.permits.try_acquire() else {
            self.starvations.fetch_add(1, Ordering::Relaxed);
            return None;
        };
        permit.forget();
        self.pop_cleared()
    }

    /// Waits for a buffer rather than reporting there is none.
    ///
    /// Backpressure is the point of this pool, but a caller that can only poll for it has to
    /// choose between spinning and sleeping past the moment a buffer came free.
    pub async fn acquire(&self) -> Option<ChunkBuffer> {
        if self.permits.available_permits() == 0 {
            self.starvations.fetch_add(1, Ordering::Relaxed);
        }
        let permit = self.permits.acquire().await.ok()?;
        permit.forget();
        self.pop_cleared()
    }

    pub fn release(&self, mut buffer: ChunkBuffer) {
        buffer.clear();
        if self.idle.push(buffer).is_ok() {
            self.permits.add_permits(1);
        }
    }

    /// How often a caller found the pool empty, which is how often the payload outran the lanes.
    pub fn starvations(&self) -> u64 {
        self.starvations.load(Ordering::Relaxed)
    }

    /// Bytes this pool holds, alignment padding included.
    pub fn footprint(&self) -> usize {
        self.permits.available_permits().max(self.idle.capacity())
            * crate::io::buffer_footprint(self.buffer_capacity)
    }

    fn pop_cleared(&self) -> Option<ChunkBuffer> {
        let mut buffer = self.idle.pop()?;
        buffer.clear();
        Some(buffer)
    }

    pub fn idle(&self) -> usize {
        self.idle.len()
    }

    pub const fn buffer_capacity(&self) -> usize {
        self.buffer_capacity
    }
}
