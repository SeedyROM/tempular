use std::sync::atomic::AtomicUsize;

pub struct AtomicCounter {
    value: AtomicUsize,
}

impl AtomicCounter {
    pub fn new() -> Self {
        Self {
            value: AtomicUsize::new(0),
        }
    }

    pub fn increment(&self) {
        self.value.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn decrement(&self) {
        self.value.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn get(&self) -> usize {
        self.value.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use rand::Rng;
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn test_atomic_counter() {
        // Your existing single-threaded test is good, keep it
        let counter = AtomicCounter::new();
        assert_eq!(counter.get(), 0);
        counter.increment();
        assert_eq!(counter.get(), 1);
        counter.increment();
        assert_eq!(counter.get(), 2);
        counter.decrement();
        assert_eq!(counter.get(), 1);
    }

    #[tokio::test]
    async fn test_concurrent_increment_decrement() {
        let counter = Arc::new(AtomicCounter::new());
        let num_threads = 10;
        let operations_per_thread = 1000;

        let handles: Vec<_> = (0..num_threads)
            .map(|_| {
                let counter = counter.clone();
                tokio::spawn(async move {
                    for _ in 0..operations_per_thread {
                        counter.increment();
                        counter.decrement();
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.await.unwrap();
        }

        // Final value should be 0 since we have equal increments and decrements
        assert_eq!(counter.get(), 0);
    }

    #[tokio::test]
    async fn test_concurrent_random_operations() {
        let counter = Arc::new(AtomicCounter::new());
        let num_threads = 8;
        let operations_per_thread = 2000;

        // For each thread:
        // - Total operations = operations_per_thread * 2
        // - Expected increment operations = total_ops * 0.75
        // - Expected decrement operations = total_ops * 0.25
        // - Net contribution = (total_ops * 0.75) - (total_ops * 0.25)
        //                    = total_ops * (0.75 - 0.25)
        //                    = total_ops * 0.5
        let expected_per_thread = (operations_per_thread * 2) as f64 * (0.75 - 0.25);
        let expected_final_value = (expected_per_thread * num_threads as f64) as usize;

        let handles: Vec<_> = (0..num_threads)
            .map(|_| {
                let counter = counter.clone();

                tokio::spawn(async move {
                    let mut rng = rand::rng();

                    for _ in 0..operations_per_thread * 2 {
                        if rng.random_bool(0.75) {
                            // 75% chance to increment
                            counter.increment();
                        } else {
                            counter.decrement();
                        }
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.await.unwrap();
        }

        // Allow a small delay for any pending operations
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Get the actual final value
        let final_value = counter.get();

        // Due to the random nature of the test, we should allow for some variance
        let tolerance = (expected_final_value as f64 * 0.1) as usize; // 10% tolerance
        assert!(
            (final_value as i64 - expected_final_value as i64).abs() <= tolerance as i64,
            "Final value {} was not within tolerance of expected value {}",
            final_value,
            expected_final_value
        );
    }

    #[tokio::test]
    async fn test_concurrent_high_contention() {
        let counter = Arc::new(AtomicCounter::new());
        let num_threads = num_cpus::get() * 2; // Use 2x CPU cores to ensure contention
        let operations_per_thread = 10_000;

        let handles: Vec<_> = (0..num_threads)
            .map(|thread_id| {
                let counter = counter.clone();
                tokio::spawn(async move {
                    // Even threads increment, odd threads decrement
                    if thread_id % 2 == 0 {
                        for _ in 0..operations_per_thread {
                            counter.increment();
                        }
                    } else {
                        for _ in 0..operations_per_thread {
                            counter.decrement();
                        }
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.await.unwrap();
        }

        // Final value should be 0 as we have equal increments and decrements
        assert_eq!(counter.get(), 0);
    }
}
