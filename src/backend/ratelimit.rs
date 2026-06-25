use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;

/// Per-provider rate limiter ensuring we don't exceed max_rpm requests per minute.
/// Tracks a rolling window of request timestamps per provider.
pub struct RateLimiter {
    buckets: Mutex<HashMap<String, Vec<Instant>>>,
    max_rpm: u32,
    window: Duration,
}

impl RateLimiter {
    /// Create a new rate limiter with the given maximum requests per minute and custom window (for tests).
    pub fn with_window(max_rpm: u32, window: Duration) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            max_rpm,
            window,
        }
    }
    
    /// Create a new rate limiter with the given maximum requests per minute.
    pub fn new(max_rpm: u32) -> Self {
        Self::with_window(max_rpm, Duration::from_secs(60))
    }

    /// Get the current number of requests in the rolling window for a provider.
    pub async fn current_rpm(&self, provider: &str) -> u32 {
        let mut buckets = self.buckets.lock().await;
        let now = Instant::now();
        let entries = buckets.entry(provider.to_string()).or_default();
        entries.retain(|&t| now.duration_since(t) < self.window);
        entries.len() as u32
    }

    /// Get the number of remaining requests allowed for a provider in the current window.
    pub async fn remaining(&self, provider: &str) -> u32 {
        let current = self.current_rpm(provider).await;
        self.max_rpm.saturating_sub(current)
    }

    /// Get the configured max RPM.
    pub fn max_rpm(&self) -> u32 {
        self.max_rpm
    }

    /// Check if a request can proceed for the given provider.
    /// If the rate limit is close to being exceeded, this will await until the window clears.
    pub async fn check_and_wait(&self, provider: &str) {
        loop {
            let should_wait = {
                let mut buckets = self.buckets.lock().await;
                let now = Instant::now();
                let entries = buckets.entry(provider.to_string()).or_default();
                
                // Remove expired entries (outside the window)
                entries.retain(|&t| now.duration_since(t) < self.window);
                
                if entries.len() >= self.max_rpm as usize {
                    // Rate limit exceeded; determine how long to wait
                    if let Some(oldest) = entries.first() {
                        let elapsed = now.duration_since(*oldest);
                        if elapsed < self.window {
                            Some(self.window - elapsed)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    // Record this request
                    entries.push(now);
                    None
                }
            };

            if let Some(wait_time) = should_wait {
                tokio::time::sleep(wait_time + Duration::from_millis(10)).await;
                continue; // Re-check after waiting
            }

            break;
        }
    }
}

/// Cloneable handle to a shared rate limiter
#[derive(Clone)]
pub struct RateLimiterHandle {
    inner: Arc<RateLimiter>,
}

impl RateLimiterHandle {
    pub fn new(max_rpm: u32) -> Self {
        Self {
            inner: Arc::new(RateLimiter::new(max_rpm)),
        }
    }

    pub async fn check_and_wait(&self, provider: &str) {
        self.inner.check_and_wait(provider).await;
    }

    pub async fn current_rpm(&self, provider: &str) -> u32 {
        self.inner.current_rpm(provider).await
    }

    pub async fn remaining(&self, provider: &str) -> u32 {
        self.inner.remaining(provider).await
    }

    pub fn max_rpm(&self) -> u32 {
        self.inner.max_rpm()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_rate_limiter_allows_under_limit() {
        let limiter = RateLimiter::new(40);
        limiter.check_and_wait("test_provider").await;
        // Should not panic or hang
    }

    #[tokio::test]
    async fn test_rate_limiter_blocks_over_limit() {
        // Use a 200ms window so the test completes fast
        let limiter = RateLimiter::with_window(2, Duration::from_millis(200));
        let start = Instant::now();
        
        // First two should pass immediately
        limiter.check_and_wait("test_provider").await;
        limiter.check_and_wait("test_provider").await;

        // Third should wait ~200ms before proceeding
        limiter.check_and_wait("test_provider").await;
        
        let elapsed = start.elapsed();
        assert!(elapsed >= Duration::from_millis(150), "Elapsed time was {:?}", elapsed);
    }
}
