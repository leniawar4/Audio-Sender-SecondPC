//! Lock-free ring buffer for audio samples
//!
//! This implements a single-producer single-consumer (SPSC) ring buffer
//! optimized for real-time audio with minimal latency.

use crossbeam::queue::ArrayQueue;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Audio frame containing interleaved samples
#[derive(Clone)]
pub struct AudioFrame {
    /// Interleaved audio samples (f32)
    pub samples: Vec<f32>,
    /// Number of channels
    pub channels: u16,
    /// Timestamp in microseconds
    pub timestamp: u64,
    /// Frame sequence number
    pub sequence: u32,
}

impl AudioFrame {
    pub fn new(samples: Vec<f32>, channels: u16, timestamp: u64, sequence: u32) -> Self {
        Self {
            samples,
            channels,
            timestamp,
            sequence,
        }
    }
    
    /// Get number of samples per channel
    pub fn samples_per_channel(&self) -> usize {
        self.samples.len() / self.channels as usize
    }
    
    /// Get frame duration in microseconds
    pub fn duration_us(&self, sample_rate: u32) -> u64 {
        (self.samples_per_channel() as u64 * 1_000_000) / sample_rate as u64
    }
}

/// Lock-free ring buffer for audio frames
pub struct RingBuffer {
    queue: ArrayQueue<AudioFrame>,
    overflow_count: AtomicUsize,
    underrun_count: AtomicUsize,
}

impl RingBuffer {
    /// Create a new ring buffer with the specified capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            queue: ArrayQueue::new(capacity),
            overflow_count: AtomicUsize::new(0),
            underrun_count: AtomicUsize::new(0),
        }
    }
    
    /// Push a frame into the buffer
    /// Returns false if buffer is full (overflow)
    pub fn push(&self, frame: AudioFrame) -> bool {
        match self.queue.push(frame) {
            Ok(()) => true,
            Err(_) => {
                self.overflow_count.fetch_add(1, Ordering::Relaxed);
                false
            }
        }
    }
    
    /// Pop a frame from the buffer
    /// Returns None if buffer is empty (underrun)
    pub fn pop(&self) -> Option<AudioFrame> {
        match self.queue.pop() {
            Some(frame) => Some(frame),
            None => {
                self.underrun_count.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }
    
    /// Try to pop without counting underrun
    pub fn try_pop(&self) -> Option<AudioFrame> {
        self.queue.pop()
    }
    
    /// Check if buffer is empty
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
    
    /// Check if buffer is full
    pub fn is_full(&self) -> bool {
        self.queue.is_full()
    }
    
    /// Get current buffer length
    pub fn len(&self) -> usize {
        self.queue.len()
    }
    
    /// Get buffer capacity
    pub fn capacity(&self) -> usize {
        self.queue.capacity()
    }
    
    /// Get overflow count
    pub fn overflow_count(&self) -> usize {
        self.overflow_count.load(Ordering::Relaxed)
    }
    
    /// Get underrun count
    pub fn underrun_count(&self) -> usize {
        self.underrun_count.load(Ordering::Relaxed)
    }
    
    /// Reset statistics
    pub fn reset_stats(&self) {
        self.overflow_count.store(0, Ordering::Relaxed);
        self.underrun_count.store(0, Ordering::Relaxed);
    }
    
    /// Get fill level as percentage
    pub fn fill_level(&self) -> f32 {
        self.len() as f32 / self.capacity() as f32
    }
}

/// Thread-safe handle to a ring buffer
pub type SharedRingBuffer = Arc<RingBuffer>;

/// Create a new shared ring buffer
pub fn create_shared_buffer(capacity: usize) -> SharedRingBuffer {
    Arc::new(RingBuffer::new(capacity))
}

/// Jitter buffer for packet reordering
pub struct JitterBuffer {
    /// Buffer slots indexed by sequence modulo capacity
    slots: Vec<Option<AudioFrame>>,
    /// Capacity (must be power of 2)
    capacity: usize,
    /// Mask for fast modulo
    mask: usize,
    /// Next expected sequence number
    next_sequence: u32,
    /// Minimum buffer delay in frames
    min_delay: usize,
    /// Current buffer level
    level: AtomicUsize,
    /// Packets received
    received: AtomicUsize,
    /// Packets lost
    lost: AtomicUsize,
    /// Late packets
    late: AtomicUsize,
}

impl JitterBuffer {
    /// Create a new jitter buffer
    /// capacity must be a power of 2
    pub fn new(capacity: usize, min_delay: usize) -> Self {
        assert!(capacity.is_power_of_two(), "Capacity must be power of 2");
        
        let mut slots = Vec::with_capacity(capacity);
        slots.resize_with(capacity, || None);
        
        Self {
            slots,
            capacity,
            mask: capacity - 1,
            next_sequence: 0,
            min_delay,
            level: AtomicUsize::new(0),
            received: AtomicUsize::new(0),
            lost: AtomicUsize::new(0),
            late: AtomicUsize::new(0),
        }
    }
    
    /// Insert a frame into the jitter buffer
    pub fn insert(&mut self, frame: AudioFrame) -> bool {
        let seq = frame.sequence;
        
        // Check if packet is too late
        if seq < self.next_sequence {
            let diff = self.next_sequence - seq;
            if diff > self.capacity as u32 / 2 {
                // Sequence wrapped around, this is actually a future packet
            } else {
                // Packet is late
                self.late.fetch_add(1, Ordering::Relaxed);
                return false;
            }
        }
        
        let index = (seq as usize) & self.mask;
        self.slots[index] = Some(frame);
        self.received.fetch_add(1, Ordering::Relaxed);
        self.level.fetch_add(1, Ordering::Relaxed);
        
        true
    }
    
    /// Get the next frame if available and buffered enough
    pub fn get_next(&mut self) -> Option<AudioFrame> {
        // Check if we have minimum delay buffered
        if self.level.load(Ordering::Relaxed) < self.min_delay {
            return None;
        }
        
        let index = (self.next_sequence as usize) & self.mask;
        let frame = self.slots[index].take();
        
        if frame.is_some() {
            self.level.fetch_sub(1, Ordering::Relaxed);
        } else {
            // Packet was lost
            self.lost.fetch_add(1, Ordering::Relaxed);
        }
        
        self.next_sequence = self.next_sequence.wrapping_add(1);
        frame
    }
    
    /// Force get the next frame even if buffer level is low
    pub fn force_get_next(&mut self) -> Option<AudioFrame> {
        let index = (self.next_sequence as usize) & self.mask;
        let frame = self.slots[index].take();
        
        if frame.is_some() {
            let _ = self.level.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                if v > 0 { Some(v - 1) } else { Some(0) }
            });
        } else {
            self.lost.fetch_add(1, Ordering::Relaxed);
        }
        
        self.next_sequence = self.next_sequence.wrapping_add(1);
        frame
    }
    
    /// Reset the jitter buffer
    pub fn reset(&mut self) {
        for slot in &mut self.slots {
            *slot = None;
        }
        self.next_sequence = 0;
        self.level.store(0, Ordering::Relaxed);
    }
    
    /// Set the next expected sequence (for sync)
    pub fn set_next_sequence(&mut self, seq: u32) {
        self.reset();
        self.next_sequence = seq;
    }
    
    /// Get statistics
    pub fn stats(&self) -> JitterBufferStats {
        JitterBufferStats {
            level: self.level.load(Ordering::Relaxed),
            capacity: self.capacity,
            received: self.received.load(Ordering::Relaxed),
            lost: self.lost.load(Ordering::Relaxed),
            late: self.late.load(Ordering::Relaxed),
        }
    }
}

/// Jitter buffer statistics
#[derive(Debug, Clone)]
pub struct JitterBufferStats {
    pub level: usize,
    pub capacity: usize,
    pub received: usize,
    pub lost: usize,
    pub late: usize,
}

impl JitterBufferStats {
    pub fn loss_rate(&self) -> f32 {
        if self.received == 0 {
            0.0
        } else {
            self.lost as f32 / (self.received + self.lost) as f32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_ring_buffer_basic() {
        let buffer = RingBuffer::new(4);
        
        let frame1 = AudioFrame::new(vec![0.0; 480], 2, 0, 0);
        let frame2 = AudioFrame::new(vec![1.0; 480], 2, 10000, 1);
        
        assert!(buffer.push(frame1));
        assert!(buffer.push(frame2));
        assert_eq!(buffer.len(), 2);
        
        let popped = buffer.pop().unwrap();
        assert_eq!(popped.sequence, 0);
        
        let popped = buffer.pop().unwrap();
        assert_eq!(popped.sequence, 1);
        
        assert!(buffer.is_empty());
    }
    
    #[test]
    fn test_jitter_buffer() {
        let mut jitter = JitterBuffer::new(16, 2);
        
        // Insert out of order
        jitter.insert(AudioFrame::new(vec![], 2, 20000, 2));
        jitter.insert(AudioFrame::new(vec![], 2, 0, 0));
        jitter.insert(AudioFrame::new(vec![], 2, 10000, 1));
        
        // Should get them in order
        let f0 = jitter.get_next().unwrap();
        assert_eq!(f0.sequence, 0);
        
        let f1 = jitter.get_next().unwrap();
        assert_eq!(f1.sequence, 1);
        
        // Not enough buffered for min_delay now
        assert!(jitter.get_next().is_none());
    }
}
