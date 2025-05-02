
use std::sync::atomic::Ordering;

pub mod allocator;

pub use allocator::SaaTrackingAllocator;



/// Gets the total number of bytes currently allocated via the global allocator.
/// This function provides a way to monitor memory usage in the application.
/// ## Returns
/// The total number of bytes currently allocated. 
pub fn get_currently_allocated_bytes() -> usize {
    allocator::ALLOCATED_BYTES.load(Ordering::Relaxed)
}