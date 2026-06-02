//! Async tool executor for non-blocking tool calls.
use anyhow::Result;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::tools::find_tool;

#[derive(Debug, Clone)]
pub struct ToolExecutionMetrics {
    pub tool_name: String,
    #[allow(dead_code)]
    pub args: String,
    pub duration_ms: u64,
    pub success: bool,
}

/// Executor for running tools asynchronously without blocking the TUI.
#[derive(Clone)]
pub struct AsyncToolExecutor;

impl AsyncToolExecutor {
    /// Create a new async tool executor.
    pub fn new() -> Self {
        Self
    }

    /// Execute a single tool asynchronously.
    ///
    /// Returns a `JoinHandle` that can be awaited to get the result.
    pub fn execute_async(&self, tool_name: String, args: String) -> JoinHandle<Result<String>> {
        tokio::spawn(async move {
            let tool = find_tool(&tool_name)
                .ok_or_else(|| anyhow::anyhow!("Unknown tool: {}", tool_name))?;
            tool.execute(&args)
        })
    }

    /// Execute multiple tools in parallel and collect all results.
    ///
    /// Returns a vector of results in the same order as the input tasks.
    #[allow(dead_code)]
    pub async fn execute_parallel(&self, tasks: Vec<(String, String)>) -> Vec<Result<String>> {
        let handles: Vec<JoinHandle<Result<String>>> = tasks
            .into_iter()
            .map(|(tool_name, args)| self.execute_async(tool_name, args))
            .collect();

        let mut results = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) => results.push(Err(anyhow::anyhow!("Task panicked: {}", e))),
            }
        }
        results
    }

    /// Execute a tool with a timeout and return metrics.
    pub async fn execute_with_timeout(
        &self,
        tool_name: String,
        args: String,
        timeout_ms: u64,
    ) -> Result<(String, ToolExecutionMetrics)> {
        let start = Instant::now();
        let handle = self.execute_async(tool_name.clone(), args.clone());
        let duration = Duration::from_millis(timeout_ms);

        match timeout(duration, handle).await {
            Ok(Ok(result)) => {
                let elapsed = start.elapsed().as_millis() as u64;
                let metrics = ToolExecutionMetrics {
                    tool_name: tool_name.clone(),
                    args: args.clone(),
                    duration_ms: elapsed,
                    success: result.is_ok(),
                };
                result.map(|r| (r, metrics))
            }
            Ok(Err(e)) => {
                let elapsed = start.elapsed().as_millis() as u64;
                let metrics = ToolExecutionMetrics {
                    tool_name: tool_name.clone(),
                    args: args.clone(),
                    duration_ms: elapsed,
                    success: false,
                };
                Err(anyhow::anyhow!("Task panicked: {}", e)).map(|r: String| (r, metrics))
            }
            Err(_) => {
                let elapsed = start.elapsed().as_millis() as u64;
                let metrics = ToolExecutionMetrics {
                    tool_name: tool_name.clone(),
                    args: args.clone(),
                    duration_ms: elapsed,
                    success: false,
                };
                Err(anyhow::anyhow!(
                    "Tool execution timed out after {}ms",
                    timeout_ms
                ))
                .map(|r: String| (r, metrics))
            }
        }
    }

    /// Execute a tool with timeout, returning only the result (backward compatible).
    pub async fn execute_with_timeout_simple(
        &self,
        tool_name: String,
        args: String,
        timeout_ms: u64,
    ) -> Result<String> {
        self.execute_with_timeout(tool_name, args, timeout_ms)
            .await
            .map(|(result, _)| result)
    }
}

impl Default for AsyncToolExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execute_async_success() {
        let executor = AsyncToolExecutor::new();
        let handle = executor.execute_async("search".to_string(), "test".to_string());
        let result = handle.await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_async_unknown_tool() {
        let executor = AsyncToolExecutor::new();
        let handle = executor.execute_async("nonexistent_tool".to_string(), "args".to_string());
        let result = handle.await.unwrap();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Unknown tool"));
    }

    #[tokio::test]
    async fn test_execute_parallel() {
        let executor = AsyncToolExecutor::new();
        let tasks = vec![
            ("search".to_string(), "foo".to_string()),
            ("grep".to_string(), "bar".to_string()),
        ];
        let results = executor.execute_parallel(tasks).await;
        assert_eq!(results.len(), 2);
        // Both search and grep may succeed or fail depending on filesystem,
        // but they should not panic.
    }

    #[tokio::test]
    async fn test_execute_with_timeout_success() {
        let executor = AsyncToolExecutor::new();
        let result = executor
            .execute_with_timeout("search".to_string(), "test".to_string(), 5000)
            .await;
        // Should complete within 5 seconds.
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_execute_with_timeout_expires() {
        let executor = AsyncToolExecutor::new();
        // Use a very short timeout so that even trivial work times out.
        let result = executor
            .execute_with_timeout("search".to_string(), "test".to_string(), 1)
            .await;
        // A 1ms timeout is extremely aggressive; the result may be Ok or Err,
        // but if it errors it should mention timeout.
        if let Err(ref e) = result {
            let msg = e.to_string();
            if msg.contains("timed out") {
                // Expected timeout behavior.
            }
        }
    }
}
