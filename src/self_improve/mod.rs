#![allow(dead_code)]

use crate::config::Config;
use crate::memory::{MemoryStore, ModelTrendData, Session, SessionQualityMetrics};
use anyhow::Result;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Analysis {
    pub model_performance: Vec<ModelStats>,
    pub tool_performance: Vec<ToolStats>,
    pub model_trends: Vec<ModelTrend>,
    pub prompt_effectiveness: Vec<PromptEffectiveness>,
    pub tool_failure_patterns: Vec<ToolFailurePattern>,
    #[allow(dead_code)]
    pub common_errors: Vec<CommonError>,
    pub session_quality: Vec<SessionQuality>,
    pub recommendations: Vec<String>,
    pub config_optimizations: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ModelStats {
    #[allow(dead_code)]
    pub model: String,
    pub sessions: usize,
    pub tool_success: usize,
    pub tool_total: usize,
    pub success_rate: f64,
    pub trend: TrendDirection,
    pub trend_delta: f64,
}

#[derive(Debug, Clone)]
pub struct ToolStats {
    pub tool_name: String,
    pub invocations: usize,
    pub success: usize,
    pub success_rate: f64,
}

#[derive(Debug, Clone)]
pub struct ModelTrend {
    #[allow(dead_code)]
    pub model: String,
    pub data_points: Vec<TrendDataPoint>,
    pub overall_direction: TrendDirection,
    pub improvement_rate: f64,
}

#[derive(Debug, Clone)]
pub struct TrendDataPoint {
    #[allow(dead_code)]
    pub day: String,
    #[allow(dead_code)]
    pub session_count: usize,
    #[allow(dead_code)]
    pub total_calls: usize,
    pub success_rate: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TrendDirection {
    Improving,
    Declining,
    Stable,
    InsufficientData,
}

#[derive(Debug, Clone)]
pub struct PromptEffectiveness {
    pub prompt_preview: String,
    #[allow(dead_code)]
    pub total_calls: usize,
    #[allow(dead_code)]
    pub success_calls: usize,
    pub success_rate: f64,
}

#[derive(Debug, Clone)]
pub struct ToolFailurePattern {
    pub tool_a: String,
    pub tool_b: String,
    pub co_failure_count: usize,
}

#[derive(Debug, Clone)]
pub struct CommonError {
    pub error_message: String,
    pub occurrence_count: usize,
}

#[derive(Debug, Clone)]
pub struct SessionQuality {
    #[allow(dead_code)]
    pub session_id: String,
    #[allow(dead_code)]
    pub model: String,
    #[allow(dead_code)]
    pub task_type: String,
    #[allow(dead_code)]
    pub message_count: usize,
    pub tool_call_count: usize,
    #[allow(dead_code)]
    pub tool_success_rate: f64,
    pub quality_score: f64,
    pub quality_label: QualityLabel,
}

#[derive(Debug, Clone, PartialEq)]
pub enum QualityLabel {
    Excellent,
    Good,
    Average,
    Poor,
    NoData,
}

pub async fn trigger_analysis(config: &Config) -> Result<()> {
    let memory = MemoryStore::new(&config.memory_db_path)?;
    let sessions = memory.get_recent_sessions(100)?;

    println!("🦈 Self-Improvement Analysis");
    println!("Analyzing last {} sessions...", sessions.len());
    println!();

    if sessions.is_empty() {
        println!("No session history found. Run some sessions first to generate insights.");
        return Ok(());
    }

    let analysis = analyze_sessions(&memory, &sessions)?;

    store_analysis_results(&memory, &analysis)?;

    print_analysis(&analysis, config);

    Ok(())
}

fn analyze_sessions(memory: &MemoryStore, sessions: &[Session]) -> Result<Analysis> {
    let model_performance = compute_model_performance(memory, sessions)?;
    let tool_performance = compute_tool_performance(memory, sessions)?;
    let model_trends = compute_model_trends(memory)?;
    let prompt_effectiveness = compute_prompt_effectiveness(memory)?;
    let tool_failure_patterns = compute_tool_failure_patterns(memory)?;
    let common_errors = compute_common_errors(memory)?;
    let session_quality = compute_session_quality(memory)?;
    let recommendations = generate_recommendations(
        &model_performance,
        &tool_performance,
        &model_trends,
        &tool_failure_patterns,
        &common_errors,
        &session_quality,
    );
    let config_optimizations =
        generate_config_optimizations(&model_performance, &tool_performance, &prompt_effectiveness);

    Ok(Analysis {
        model_performance,
        tool_performance,
        model_trends,
        prompt_effectiveness,
        tool_failure_patterns,
        common_errors,
        session_quality,
        recommendations,
        config_optimizations,
    })
}

fn compute_model_performance(
    memory: &MemoryStore,
    sessions: &[Session],
) -> Result<Vec<ModelStats>> {
    let mut model_stats: HashMap<String, (usize, usize, usize)> = HashMap::new();

    for session in sessions {
        let tool_calls = memory.search_tool_calls_by_session(&session.id)?;
        let success_count = tool_calls.iter().filter(|tc| tc.success).count();
        let total_count = tool_calls.len();

        let entry = model_stats
            .entry(session.model.clone())
            .or_insert((0, 0, 0));
        entry.0 += 1;
        entry.1 += success_count;
        entry.2 += total_count;
    }

    let trends_data = memory.get_model_performance_trends(1000)?;

    let model_performance: Vec<ModelStats> = model_stats
        .iter()
        .map(|(model, (sessions_count, success, total))| {
            let success_rate = if *total > 0 {
                (*success as f64 / *total as f64) * 100.0
            } else {
                0.0
            };
            let (trend, trend_delta) = compute_trend_for_model(model, &trends_data);

            ModelStats {
                model: model.clone(),
                sessions: *sessions_count,
                tool_success: *success,
                tool_total: *total,
                success_rate,
                trend,
                trend_delta,
            }
        })
        .collect();

    Ok(model_performance)
}

fn compute_trend_for_model(model: &str, trends_data: &[ModelTrendData]) -> (TrendDirection, f64) {
    let model_trends: Vec<&ModelTrendData> =
        trends_data.iter().filter(|t| t.model == model).collect();

    if model_trends.len() < 2 {
        return (TrendDirection::InsufficientData, 0.0);
    }

    let mut sorted = model_trends.clone();
    sorted.sort_by(|a, b| a.day.cmp(&b.day));

    let first = sorted.first().map(|t| t.success_rate).unwrap_or(0.0);
    let last = sorted.last().map(|t| t.success_rate).unwrap_or(0.0);
    let delta = last - first;

    if delta > 5.0 {
        (TrendDirection::Improving, delta)
    } else if delta < -5.0 {
        (TrendDirection::Declining, delta)
    } else {
        (TrendDirection::Stable, delta)
    }
}

fn compute_tool_performance(memory: &MemoryStore, sessions: &[Session]) -> Result<Vec<ToolStats>> {
    let mut tool_stats: HashMap<String, (usize, usize)> = HashMap::new();

    for session in sessions {
        let tool_calls = memory.search_tool_calls_by_session(&session.id)?;
        for tc in &tool_calls {
            let entry = tool_stats.entry(tc.tool_name.clone()).or_insert((0, 0));
            entry.0 += 1;
            if tc.success {
                entry.1 += 1;
            }
        }
    }

    let tool_performance: Vec<ToolStats> = tool_stats
        .iter()
        .map(|(tool, (total, success))| ToolStats {
            tool_name: tool.clone(),
            invocations: *total,
            success: *success,
            success_rate: if *total > 0 {
                (*success as f64 / *total as f64) * 100.0
            } else {
                0.0
            },
        })
        .collect();

    Ok(tool_performance)
}

fn compute_model_trends(memory: &MemoryStore) -> Result<Vec<ModelTrend>> {
    let trends_data = memory.get_model_performance_trends(1000)?;
    let mut by_model: HashMap<String, Vec<&ModelTrendData>> = HashMap::new();

    for data in &trends_data {
        by_model.entry(data.model.clone()).or_default().push(data);
    }

    let mut model_trends = Vec::new();
    for (model, data_points) in by_model {
        if data_points.len() < 2 {
            continue;
        }

        let mut sorted = data_points.clone();
        sorted.sort_by(|a, b| a.day.cmp(&b.day));

        let trend_points: Vec<TrendDataPoint> = sorted
            .iter()
            .map(|d| TrendDataPoint {
                day: d.day.clone(),
                session_count: d.session_count,
                total_calls: d.total_calls,
                success_rate: d.success_rate,
            })
            .collect();

        let first_rate = trend_points.first().map(|p| p.success_rate).unwrap_or(0.0);
        let last_rate = trend_points.last().map(|p| p.success_rate).unwrap_or(0.0);
        let overall_direction = if last_rate - first_rate > 5.0 {
            TrendDirection::Improving
        } else if last_rate - first_rate < -5.0 {
            TrendDirection::Declining
        } else {
            TrendDirection::Stable
        };

        let days = trend_points.len().max(1) as f64;
        let improvement_rate = (last_rate - first_rate) / days;

        model_trends.push(ModelTrend {
            model,
            data_points: trend_points,
            overall_direction,
            improvement_rate,
        });
    }

    Ok(model_trends)
}

fn compute_prompt_effectiveness(memory: &MemoryStore) -> Result<Vec<PromptEffectiveness>> {
    let raw_data = memory.get_prompt_effectiveness(50)?;

    let effectiveness: Vec<PromptEffectiveness> = raw_data
        .into_iter()
        .map(|(prompt, total, success, rate)| {
            let preview = if prompt.len() > 80 {
                format!("{}...", &prompt[..80])
            } else {
                prompt
            };
            PromptEffectiveness {
                prompt_preview: preview,
                total_calls: total,
                success_calls: success,
                success_rate: rate,
            }
        })
        .collect();

    Ok(effectiveness)
}

fn compute_tool_failure_patterns(memory: &MemoryStore) -> Result<Vec<ToolFailurePattern>> {
    let raw_patterns = memory.get_tool_failure_patterns(20)?;

    let patterns: Vec<ToolFailurePattern> = raw_patterns
        .into_iter()
        .map(|(tool_a, tool_b, count)| ToolFailurePattern {
            tool_a,
            tool_b,
            co_failure_count: count,
        })
        .collect();

    Ok(patterns)
}

fn compute_common_errors(memory: &MemoryStore) -> Result<Vec<CommonError>> {
    let raw_errors = memory.get_common_errors(20)?;

    let errors: Vec<CommonError> = raw_errors
        .into_iter()
        .map(|(msg, count)| {
            let preview = if msg.len() > 100 {
                format!("{}...", &msg[..100])
            } else {
                msg
            };
            CommonError {
                error_message: preview,
                occurrence_count: count,
            }
        })
        .collect();

    Ok(errors)
}

fn compute_session_quality(memory: &MemoryStore) -> Result<Vec<SessionQuality>> {
    let raw_metrics = memory.get_session_quality_metrics(100)?;

    let mut session_qualities = Vec::new();
    for mut metrics in raw_metrics {
        let quality_score = calculate_quality_score(&metrics);
        let quality_label = score_to_label(quality_score, metrics.tool_call_count);
        metrics.quality_score = quality_score;

        session_qualities.push(SessionQuality {
            session_id: metrics.session_id,
            model: metrics.model,
            task_type: metrics.task_type,
            message_count: metrics.message_count,
            tool_call_count: metrics.tool_call_count,
            tool_success_rate: metrics.tool_success_rate,
            quality_score,
            quality_label,
        });
    }

    Ok(session_qualities)
}

fn calculate_quality_score(metrics: &SessionQualityMetrics) -> f64 {
    if metrics.tool_call_count == 0 {
        if metrics.message_count > 0 {
            return 50.0;
        }
        return 0.0;
    }

    let tool_score = metrics.tool_success_rate;
    let efficiency_score = if metrics.message_count > 0 {
        (metrics.tool_call_count as f64 / metrics.message_count as f64) * 100.0
    } else {
        0.0
    }
    .min(100.0);

    let volume_bonus = if metrics.tool_call_count >= 5 {
        5.0
    } else {
        0.0
    };

    (tool_score * 0.7 + efficiency_score * 0.3 + volume_bonus).min(100.0)
}

fn score_to_label(score: f64, tool_call_count: usize) -> QualityLabel {
    if tool_call_count == 0 && score == 0.0 {
        QualityLabel::NoData
    } else if score >= 80.0 {
        QualityLabel::Excellent
    } else if score >= 60.0 {
        QualityLabel::Good
    } else if score >= 40.0 {
        QualityLabel::Average
    } else {
        QualityLabel::Poor
    }
}

fn generate_recommendations(
    model_performance: &[ModelStats],
    tool_performance: &[ToolStats],
    model_trends: &[ModelTrend],
    tool_failure_patterns: &[ToolFailurePattern],
    common_errors: &[CommonError],
    session_quality: &[SessionQuality],
) -> Vec<String> {
    let mut recommendations = Vec::new();

    if model_performance.len() > 1 {
        let best = model_performance.iter().max_by(|a, b| {
            a.success_rate
                .partial_cmp(&b.success_rate)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let worst = model_performance.iter().min_by(|a, b| {
            a.success_rate
                .partial_cmp(&b.success_rate)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if let (Some(best), Some(worst)) = (best, worst) {
            if best.success_rate - worst.success_rate > 20.0 {
                recommendations.push(format!(
                    "Model '{}' outperforms '{}' by {:.1} percentage points in tool success rate. Consider routing more tasks to '{}'.",
                    best.model, worst.model, best.success_rate - worst.success_rate, best.model
                ));
            }
        }
    }

    for trend in model_trends {
        match trend.overall_direction {
            TrendDirection::Improving => {
                recommendations.push(format!(
                    "Model '{}' is improving at {:.2} percentage points per day. Keep current configuration.",
                    trend.model, trend.improvement_rate
                ));
            }
            TrendDirection::Declining => {
                recommendations.push(format!(
                    "Model '{}' performance is declining at {:.2} percentage points per day. Review recent changes.",
                    trend.model, trend.improvement_rate.abs()
                ));
            }
            _ => {}
        }
    }

    let high_volume_tools: Vec<_> = tool_performance
        .iter()
        .filter(|t| t.invocations >= 5)
        .collect();

    if !high_volume_tools.is_empty() {
        let avg_success = high_volume_tools
            .iter()
            .map(|t| t.success_rate)
            .sum::<f64>()
            / high_volume_tools.len() as f64;
        let below_avg: Vec<_> = high_volume_tools
            .iter()
            .filter(|t| t.success_rate < avg_success * 0.8)
            .collect();

        if !below_avg.is_empty() {
            recommendations.push(format!(
                "High-volume tools average {:.1}% success rate. {} tools are significantly below average.",
                avg_success, below_avg.len()
            ));
        }
    }

    for pattern in tool_failure_patterns {
        recommendations.push(format!(
            "Tools '{}' and '{}' fail together {} times. They may share a dependency or configuration issue.",
            pattern.tool_a, pattern.tool_b, pattern.co_failure_count
        ));
    }

    if !common_errors.is_empty() {
        let top_error = &common_errors[0];
        recommendations.push(format!(
            "Most common error ({} occurrences): {}. Consider adding error handling for this case.",
            top_error.occurrence_count, top_error.error_message
        ));
    }

    let poor_sessions: Vec<_> = session_quality
        .iter()
        .filter(|s| s.quality_label == QualityLabel::Poor && s.tool_call_count > 0)
        .collect();

    if !poor_sessions.is_empty() {
        recommendations.push(format!(
            "{} sessions scored poorly on quality metrics. Review tool usage patterns in these sessions.",
            poor_sessions.len()
        ));
    }

    if recommendations.is_empty() {
        recommendations.push("All systems performing within expected parameters.".to_string());
    }

    recommendations
}

fn generate_config_optimizations(
    model_performance: &[ModelStats],
    tool_performance: &[ToolStats],
    prompt_effectiveness: &[PromptEffectiveness],
) -> Vec<String> {
    let mut optimizations = Vec::new();

    let best_model = model_performance
        .iter()
        .max_by(|a, b| {
            a.success_rate
                .partial_cmp(&b.success_rate)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|s| s.model.clone());

    if let Some(best) = best_model {
        optimizations.push(format!(
            "Best performing model: '{}' with {:.1}% tool success rate",
            best,
            model_performance
                .iter()
                .find(|m| m.model == best)
                .map(|m| m.success_rate)
                .unwrap_or(0.0)
        ));
    }

    let underperforming: Vec<_> = tool_performance
        .iter()
        .filter(|t| t.success_rate < 50.0 && t.invocations >= 3)
        .collect();

    if !underperforming.is_empty() {
        optimizations.push("Underperforming tools requiring review:".to_string());
        for tool in underperforming {
            optimizations.push(format!(
                "  - {}: {:.1}% success rate over {} calls",
                tool.tool_name, tool.success_rate, tool.invocations
            ));
        }
    }

    if !prompt_effectiveness.is_empty() {
        let best_prompt = prompt_effectiveness.iter().max_by(|a, b| {
            a.success_rate
                .partial_cmp(&b.success_rate)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if let Some(best) = best_prompt {
            if best.success_rate > 70.0 && best.total_calls >= 5 {
                optimizations.push(format!(
                    "Most effective system prompt achieves {:.1}% success rate over {} calls",
                    best.success_rate, best.total_calls
                ));
            }
        }
    }

    optimizations
}

fn store_analysis_results(memory: &MemoryStore, analysis: &Analysis) -> Result<()> {
    for stat in &analysis.model_performance {
        let value = format!(
            "sessions={},success={},total={},rate={:.2},trend={:?},delta={:.2}",
            stat.sessions,
            stat.tool_success,
            stat.tool_total,
            stat.success_rate,
            stat.trend,
            stat.trend_delta
        );
        memory.save_analysis_result("model_performance", &stat.model, &value)?;
    }

    for stat in &analysis.tool_performance {
        let value = format!(
            "invocations={},success={},rate={:.2}",
            stat.invocations, stat.success, stat.success_rate
        );
        memory.save_analysis_result("tool_performance", &stat.tool_name, &value)?;
    }

    for (i, rec) in analysis.recommendations.iter().enumerate() {
        memory.save_analysis_result("recommendations", &format!("rec_{}", i), rec)?;
    }

    for (i, opt) in analysis.config_optimizations.iter().enumerate() {
        memory.save_analysis_result("config_optimizations", &format!("opt_{}", i), opt)?;
    }

    let avg_quality = if !analysis.session_quality.is_empty() {
        analysis
            .session_quality
            .iter()
            .map(|s| s.quality_score)
            .sum::<f64>()
            / analysis.session_quality.len() as f64
    } else {
        0.0
    };
    memory.save_analysis_result(
        "summary",
        "average_quality_score",
        &format!("{:.2}", avg_quality),
    )?;

    let total_sessions = analysis.session_quality.len();
    memory.save_analysis_result(
        "summary",
        "total_sessions_analyzed",
        &total_sessions.to_string(),
    )?;

    Ok(())
}

fn print_analysis(analysis: &Analysis, config: &Config) {
    println!("Model Performance:");
    println!(
        "{:<20} | {:8} | {:8} | {:8} | {:6} | {:12}",
        "Model", "Sessions", "Success", "Total", "Rate", "Trend"
    );
    println!("{}", "-".repeat(75));
    for stat in &analysis.model_performance {
        let trend_str = match stat.trend {
            TrendDirection::Improving => format!("↑ +{:.1}%", stat.trend_delta),
            TrendDirection::Declining => format!("↓ {:.1}%", stat.trend_delta),
            TrendDirection::Stable => "→ stable".to_string(),
            TrendDirection::InsufficientData => "? n/a".to_string(),
        };
        println!(
            "{:<20} | {:8} | {:8} | {:8} | {:5.1}% | {}",
            stat.model,
            stat.sessions,
            stat.tool_success,
            stat.tool_total,
            stat.success_rate,
            trend_str
        );
    }
    println!();

    println!("Tool Performance:");
    println!(
        "{:<15} | {:8} | {:8} | {:6}",
        "Tool", "Calls", "Success", "Rate"
    );
    println!("{}", "-".repeat(45));
    for stat in &analysis.tool_performance {
        println!(
            "{:<15} | {:8} | {:8} | {:5.1}%",
            stat.tool_name, stat.invocations, stat.success, stat.success_rate
        );
    }
    println!();

    if !analysis.model_trends.is_empty() {
        println!("Model Performance Trends:");
        for trend in &analysis.model_trends {
            let direction = match trend.overall_direction {
                TrendDirection::Improving => "📈 Improving",
                TrendDirection::Declining => "📉 Declining",
                TrendDirection::Stable => "➡️  Stable",
                TrendDirection::InsufficientData => "❓ Insufficient data",
            };
            println!(
                "  {}: {} ({:.2} pp/day over {} days)",
                trend.model,
                direction,
                trend.improvement_rate,
                trend.data_points.len()
            );
        }
        println!();
    }

    if !analysis.prompt_effectiveness.is_empty() {
        println!("Prompt Effectiveness (top performing):");
        let mut sorted = analysis.prompt_effectiveness.clone();
        sorted.sort_by(|a, b| {
            b.success_rate
                .partial_cmp(&a.success_rate)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for (i, pe) in sorted.iter().take(3).enumerate() {
            println!(
                "  {}. {:.1}% success over {} calls - {}",
                i + 1,
                pe.success_rate,
                pe.total_calls,
                pe.prompt_preview
            );
        }
        println!();
    }

    if !analysis.tool_failure_patterns.is_empty() {
        println!("Tool Failure Patterns:");
        for pattern in &analysis.tool_failure_patterns {
            println!(
                "  • {} + {} fail together {} times",
                pattern.tool_a, pattern.tool_b, pattern.co_failure_count
            );
        }
        println!();
    }

    if !analysis.session_quality.is_empty() {
        println!("Session Quality Distribution:");
        let excellent = analysis
            .session_quality
            .iter()
            .filter(|s| s.quality_label == QualityLabel::Excellent)
            .count();
        let good = analysis
            .session_quality
            .iter()
            .filter(|s| s.quality_label == QualityLabel::Good)
            .count();
        let average = analysis
            .session_quality
            .iter()
            .filter(|s| s.quality_label == QualityLabel::Average)
            .count();
        let poor = analysis
            .session_quality
            .iter()
            .filter(|s| s.quality_label == QualityLabel::Poor)
            .count();
        println!(
            "  Excellent: {} | Good: {} | Average: {} | Poor: {}",
            excellent, good, average, poor
        );
        println!();
    }

    println!("Recommendations:");
    for (i, rec) in analysis.recommendations.iter().enumerate() {
        println!("  {}. {}", i + 1, rec);
    }
    println!();

    println!("Config Optimization:");
    let best_model = analysis
        .model_performance
        .iter()
        .max_by(|a, b| {
            a.success_rate
                .partial_cmp(&b.success_rate)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|s| s.model.clone());

    if let Some(best) = best_model {
        if best != config.default_model {
            println!(
                "  Consider setting default model to '{}' (highest success rate)",
                best
            );
        } else {
            println!("  Default model '{}' is performing optimally", best);
        }
    }

    for opt in &analysis.config_optimizations {
        if !opt.starts_with("Best performing model") {
            println!("  {}", opt);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{MemoryStore, Message, ToolCall};
    use chrono::Utc;
    use std::path::PathBuf;
    fn create_test_memory() -> MemoryStore {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let count = COUNTER.fetch_add(1, Ordering::SeqCst);
        let db_path = format!("/tmp/openshark_test_{}_{}.db", std::process::id(), count);
        let _ = std::fs::remove_file(&db_path);
        MemoryStore::new(std::path::Path::new(&db_path)).unwrap()
    }

    fn create_test_config() -> Config {
        Config {
            version: crate::VERSION.to_string(),
            default_model: "model-a".to_string(),
            providers: std::collections::HashMap::new(),
            memory_db_path: PathBuf::from("/tmp/test.db"),
            tools_enabled: vec!["fs".to_string(), "terminal".to_string()],
            auto_route: true,
            cost_limit_usd: 10.0,
            agent: crate::config::AgentIdentity::default(),
            gateway: crate::gateway::GatewayConfig::default(),
            user_name: "user".to_string(),
            theme: "synthwave84".to_string(),
            filesystem: crate::config::FilesystemConfig::default(),
            swarm: crate::swarm::SwarmConfig::default(),
            context_compression: crate::memory::compression::ContextCompressionConfig::default(),
        }
    }

    fn seed_test_data(store: &MemoryStore) {
        store.create_session("sess-1", "model-a", "code").unwrap();
        store.create_session("sess-2", "model-a", "code").unwrap();
        store.create_session("sess-3", "model-b", "chat").unwrap();
        store.create_session("sess-4", "model-b", "chat").unwrap();

        store
            .save_message(&Message {
                id: "msg-1".to_string(),
                session_id: "sess-1".to_string(),
                role: "system".to_string(),
                content: "You are a helpful coding assistant".to_string(),
                created_at: Utc::now(),
                tokens_used: Some(100),
            })
            .unwrap();

        store
            .save_message(&Message {
                id: "msg-2".to_string(),
                session_id: "sess-1".to_string(),
                role: "user".to_string(),
                content: "Write some code".to_string(),
                created_at: Utc::now(),
                tokens_used: Some(50),
            })
            .unwrap();

        store
            .save_tool_call(&ToolCall {
                id: "tc-1".to_string(),
                session_id: "sess-1".to_string(),
                tool_name: "fs".to_string(),
                args: "{\"path\": \"/tmp\"}".to_string(),
                result: "ok".to_string(),
                success: true,
                created_at: Utc::now(),
            })
            .unwrap();

        store
            .save_tool_call(&ToolCall {
                id: "tc-2".to_string(),
                session_id: "sess-1".to_string(),
                tool_name: "terminal".to_string(),
                args: "{\"cmd\": \"ls\"}".to_string(),
                result: "ok".to_string(),
                success: true,
                created_at: Utc::now(),
            })
            .unwrap();

        store
            .save_tool_call(&ToolCall {
                id: "tc-3".to_string(),
                session_id: "sess-2".to_string(),
                tool_name: "fs".to_string(),
                args: "{\"path\": \"/tmp\"}".to_string(),
                result: "ok".to_string(),
                success: true,
                created_at: Utc::now(),
            })
            .unwrap();

        store
            .save_tool_call(&ToolCall {
                id: "tc-4".to_string(),
                session_id: "sess-3".to_string(),
                tool_name: "fs".to_string(),
                args: "{\"path\": \"/bad\"}".to_string(),
                result: "permission denied".to_string(),
                success: false,
                created_at: Utc::now(),
            })
            .unwrap();

        store
            .save_tool_call(&ToolCall {
                id: "tc-5".to_string(),
                session_id: "sess-3".to_string(),
                tool_name: "terminal".to_string(),
                args: "{\"cmd\": \"badcmd\"}".to_string(),
                result: "command not found".to_string(),
                success: false,
                created_at: Utc::now(),
            })
            .unwrap();

        store
            .save_tool_call(&ToolCall {
                id: "tc-6".to_string(),
                session_id: "sess-4".to_string(),
                tool_name: "fs".to_string(),
                args: "{\"path\": \"/bad\"}".to_string(),
                result: "permission denied".to_string(),
                success: false,
                created_at: Utc::now(),
            })
            .unwrap();

        store
            .save_tool_call(&ToolCall {
                id: "tc-7".to_string(),
                session_id: "sess-4".to_string(),
                tool_name: "terminal".to_string(),
                args: "{\"cmd\": \"badcmd2\"}".to_string(),
                result: "command not found".to_string(),
                success: false,
                created_at: Utc::now(),
            })
            .unwrap();
    }

    #[test]
    fn test_compute_model_performance() {
        let store = create_test_memory();
        seed_test_data(&store);

        let sessions = store.get_recent_sessions(100).unwrap();
        let performance = compute_model_performance(&store, &sessions).unwrap();

        assert_eq!(performance.len(), 2);

        let model_a = performance.iter().find(|p| p.model == "model-a").unwrap();
        assert_eq!(model_a.sessions, 2);
        assert_eq!(model_a.tool_total, 3);
        assert_eq!(model_a.tool_success, 3);
        assert!((model_a.success_rate - 100.0).abs() < 0.01);

        let model_b = performance.iter().find(|p| p.model == "model-b").unwrap();
        assert_eq!(model_b.sessions, 2);
        assert_eq!(model_b.tool_total, 4);
        assert_eq!(model_b.tool_success, 0);
        assert!((model_b.success_rate - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_compute_tool_performance() {
        let store = create_test_memory();
        seed_test_data(&store);

        let sessions = store.get_recent_sessions(100).unwrap();
        let performance = compute_tool_performance(&store, &sessions).unwrap();

        assert_eq!(performance.len(), 2);

        let fs = performance.iter().find(|p| p.tool_name == "fs").unwrap();
        assert_eq!(fs.invocations, 4);
        assert_eq!(fs.success, 2);
        assert!((fs.success_rate - 50.0).abs() < 0.01);

        let terminal = performance
            .iter()
            .find(|p| p.tool_name == "terminal")
            .unwrap();
        assert_eq!(terminal.invocations, 3);
        assert_eq!(terminal.success, 1);
        assert!((terminal.success_rate - 33.33).abs() < 0.1);
    }

    #[test]
    fn test_compute_session_quality() {
        let store = create_test_memory();
        seed_test_data(&store);

        let quality = compute_session_quality(&store).unwrap();
        assert_eq!(quality.len(), 4);

        let sess1 = quality.iter().find(|q| q.session_id == "sess-1").unwrap();
        assert_eq!(sess1.tool_call_count, 2);
        assert!((sess1.tool_success_rate - 100.0).abs() < 0.01);
        assert!(sess1.quality_score > 0.0);
        assert!(
            sess1.quality_label == QualityLabel::Excellent
                || sess1.quality_label == QualityLabel::Good
        );
    }

    #[test]
    fn test_calculate_quality_score() {
        let metrics = SessionQualityMetrics {
            session_id: "test".to_string(),
            model: "model-a".to_string(),
            task_type: "code".to_string(),
            started_at: Utc::now(),
            message_count: 10,
            tool_call_count: 5,
            tool_success_count: 5,
            tool_success_rate: 100.0,
            quality_score: 0.0,
        };
        let score = calculate_quality_score(&metrics);
        assert!(score > 0.0);
        assert!(score <= 100.0);
    }

    #[test]
    fn test_generate_recommendations() {
        let model_perf = vec![
            ModelStats {
                model: "model-a".to_string(),
                sessions: 10,
                tool_success: 9,
                tool_total: 10,
                success_rate: 90.0,
                trend: TrendDirection::Stable,
                trend_delta: 0.0,
            },
            ModelStats {
                model: "model-b".to_string(),
                sessions: 10,
                tool_success: 5,
                tool_total: 10,
                success_rate: 50.0,
                trend: TrendDirection::Stable,
                trend_delta: 0.0,
            },
        ];
        let tool_perf = vec![];
        let trends = vec![];
        let patterns = vec![];
        let errors = vec![];
        let quality = vec![];

        let recs = generate_recommendations(
            &model_perf,
            &tool_perf,
            &trends,
            &patterns,
            &errors,
            &quality,
        );

        assert!(!recs.is_empty());
        assert!(recs[0].contains("model-a"));
        assert!(recs[0].contains("model-b"));
    }

    #[test]
    fn test_trend_direction_computation() {
        let trends = vec![
            ModelTrendData {
                model: "model-a".to_string(),
                day: "2024-01-01".to_string(),
                session_count: 1,
                total_calls: 10,
                success_calls: 5,
                success_rate: 50.0,
            },
            ModelTrendData {
                model: "model-a".to_string(),
                day: "2024-01-02".to_string(),
                session_count: 1,
                total_calls: 10,
                success_calls: 9,
                success_rate: 90.0,
            },
        ];

        let (direction, delta) = compute_trend_for_model("model-a", &trends);
        assert_eq!(direction, TrendDirection::Improving);
        assert!((delta - 40.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_store_analysis_results() {
        let store = create_test_memory();
        seed_test_data(&store);

        let sessions = store.get_recent_sessions(100).unwrap();
        let analysis = analyze_sessions(&store, &sessions).unwrap();
        store_analysis_results(&store, &analysis).unwrap();

        let model_results = store.get_analysis_results("model_performance").unwrap();
        assert!(!model_results.is_empty());

        let rec_results = store.get_analysis_results("recommendations").unwrap();
        assert!(!rec_results.is_empty());
    }

    #[test]
    fn test_analyze_sessions_integration() {
        let store = create_test_memory();
        seed_test_data(&store);

        let sessions = store.get_recent_sessions(100).unwrap();
        let analysis = analyze_sessions(&store, &sessions).unwrap();

        assert!(!analysis.model_performance.is_empty());
        assert!(!analysis.tool_performance.is_empty());
        assert!(!analysis.recommendations.is_empty());
        assert!(!analysis.config_optimizations.is_empty());
    }

    #[test]
    fn test_empty_sessions_handling() {
        let store = create_test_memory();
        let sessions = store.get_recent_sessions(100).unwrap();
        let analysis = analyze_sessions(&store, &sessions).unwrap();

        assert!(analysis.model_performance.is_empty());
        assert!(analysis.tool_performance.is_empty());
        assert!(!analysis.recommendations.is_empty());
        assert_eq!(
            analysis.recommendations[0],
            "All systems performing within expected parameters."
        );
    }

    #[test]
    fn test_score_to_label() {
        assert_eq!(score_to_label(85.0, 5), QualityLabel::Excellent);
        assert_eq!(score_to_label(70.0, 5), QualityLabel::Good);
        assert_eq!(score_to_label(50.0, 5), QualityLabel::Average);
        assert_eq!(score_to_label(30.0, 5), QualityLabel::Poor);
        assert_eq!(score_to_label(0.0, 0), QualityLabel::NoData);
    }

    #[test]
    fn test_tool_failure_patterns() {
        let store = create_test_memory();
        seed_test_data(&store);

        let patterns = compute_tool_failure_patterns(&store).unwrap();
        assert!(!patterns.is_empty());

        let pattern = patterns
            .iter()
            .find(|p| p.tool_a == "fs" && p.tool_b == "terminal");
        assert!(pattern.is_some());
        assert_eq!(pattern.unwrap().co_failure_count, 2);
    }

    #[test]
    fn test_common_errors() {
        let store = create_test_memory();
        seed_test_data(&store);

        let errors = compute_common_errors(&store).unwrap();
        assert!(!errors.is_empty());

        let perm_error = errors
            .iter()
            .find(|e| e.error_message.contains("permission denied"));
        assert!(perm_error.is_some());
    }

    #[test]
    fn test_calculate_quality_score_no_tools() {
        let metrics_no_messages = SessionQualityMetrics {
            session_id: "test".to_string(),
            model: "model-a".to_string(),
            task_type: "code".to_string(),
            started_at: Utc::now(),
            message_count: 0,
            tool_call_count: 0,
            tool_success_count: 0,
            tool_success_rate: 0.0,
            quality_score: 0.0,
        };
        assert_eq!(calculate_quality_score(&metrics_no_messages), 0.0);

        let metrics_with_messages = SessionQualityMetrics {
            session_id: "test2".to_string(),
            model: "model-a".to_string(),
            task_type: "code".to_string(),
            started_at: Utc::now(),
            message_count: 5,
            tool_call_count: 0,
            tool_success_count: 0,
            tool_success_rate: 0.0,
            quality_score: 0.0,
        };
        assert_eq!(calculate_quality_score(&metrics_with_messages), 50.0);
    }

    #[test]
    fn test_compute_trend_for_model_insufficient_data() {
        let trends = vec![ModelTrendData {
            model: "model-a".to_string(),
            day: "2024-01-01".to_string(),
            session_count: 1,
            total_calls: 10,
            success_calls: 5,
            success_rate: 50.0,
        }];

        let (direction, delta) = compute_trend_for_model("model-a", &trends);
        assert_eq!(direction, TrendDirection::InsufficientData);
        assert_eq!(delta, 0.0);
    }

    #[test]
    fn test_compute_trend_for_model_declining() {
        let trends = vec![
            ModelTrendData {
                model: "model-a".to_string(),
                day: "2024-01-01".to_string(),
                session_count: 1,
                total_calls: 10,
                success_calls: 9,
                success_rate: 90.0,
            },
            ModelTrendData {
                model: "model-a".to_string(),
                day: "2024-01-02".to_string(),
                session_count: 1,
                total_calls: 10,
                success_calls: 1,
                success_rate: 10.0,
            },
        ];

        let (direction, delta) = compute_trend_for_model("model-a", &trends);
        assert_eq!(direction, TrendDirection::Declining);
        assert!((delta + 80.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_compute_trend_for_model_stable() {
        let trends = vec![
            ModelTrendData {
                model: "model-a".to_string(),
                day: "2024-01-01".to_string(),
                session_count: 1,
                total_calls: 10,
                success_calls: 5,
                success_rate: 50.0,
            },
            ModelTrendData {
                model: "model-a".to_string(),
                day: "2024-01-02".to_string(),
                session_count: 1,
                total_calls: 10,
                success_calls: 6,
                success_rate: 52.0,
            },
        ];

        let (direction, delta) = compute_trend_for_model("model-a", &trends);
        assert_eq!(direction, TrendDirection::Stable);
        assert!((delta - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_generate_recommendations_empty() {
        let model_perf = vec![];
        let tool_perf = vec![];
        let trends = vec![];
        let patterns = vec![];
        let errors = vec![];
        let quality = vec![];

        let recs = generate_recommendations(
            &model_perf,
            &tool_perf,
            &trends,
            &patterns,
            &errors,
            &quality,
        );
        assert_eq!(recs.len(), 1);
        assert_eq!(
            recs[0],
            "All systems performing within expected parameters."
        );
    }

    #[test]
    fn test_generate_recommendations_with_trends() {
        let model_perf = vec![
            ModelStats {
                model: "model-a".to_string(),
                sessions: 10,
                tool_success: 9,
                tool_total: 10,
                success_rate: 90.0,
                trend: TrendDirection::Stable,
                trend_delta: 0.0,
            },
            ModelStats {
                model: "model-b".to_string(),
                sessions: 10,
                tool_success: 5,
                tool_total: 10,
                success_rate: 50.0,
                trend: TrendDirection::Stable,
                trend_delta: 0.0,
            },
        ];
        let trends = vec![ModelTrend {
            model: "model-a".to_string(),
            data_points: vec![],
            overall_direction: TrendDirection::Improving,
            improvement_rate: 2.5,
        }];
        let tool_perf = vec![];
        let patterns = vec![];
        let errors = vec![];
        let quality = vec![];

        let recs = generate_recommendations(
            &model_perf,
            &tool_perf,
            &trends,
            &patterns,
            &errors,
            &quality,
        );
        assert!(recs.iter().any(|r| r.contains("improving")));
    }

    #[test]
    fn test_generate_config_optimizations() {
        let model_perf = vec![ModelStats {
            model: "model-a".to_string(),
            sessions: 10,
            tool_success: 9,
            tool_total: 10,
            success_rate: 90.0,
            trend: TrendDirection::Stable,
            trend_delta: 0.0,
        }];
        let tool_perf = vec![];
        let prompt_effectiveness = vec![];

        let opts = generate_config_optimizations(&model_perf, &tool_perf, &prompt_effectiveness);
        assert!(!opts.is_empty());
        assert!(opts[0].contains("model-a"));
    }

    #[test]
    fn test_generate_config_optimizations_underperforming_tools() {
        let model_perf = vec![];
        let tool_perf = vec![ToolStats {
            tool_name: "bad-tool".to_string(),
            invocations: 5,
            success: 1,
            success_rate: 20.0,
        }];
        let prompt_effectiveness = vec![];

        let opts = generate_config_optimizations(&model_perf, &tool_perf, &prompt_effectiveness);
        assert!(opts.iter().any(|o| o.contains("Underperforming")));
    }

    #[test]
    fn test_compute_model_trends_empty() {
        let store = create_test_memory();
        let trends = compute_model_trends(&store).unwrap();
        assert!(trends.is_empty());
    }

    #[test]
    fn test_compute_prompt_effectiveness_empty() {
        let store = create_test_memory();
        let effectiveness = compute_prompt_effectiveness(&store).unwrap();
        assert!(effectiveness.is_empty());
    }

    #[test]
    fn test_quality_label_boundaries() {
        assert_eq!(score_to_label(80.0, 5), QualityLabel::Excellent);
        assert_eq!(score_to_label(60.0, 5), QualityLabel::Good);
        assert_eq!(score_to_label(40.0, 5), QualityLabel::Average);
        assert_eq!(score_to_label(39.9, 5), QualityLabel::Poor);
    }

    #[test]
    fn test_analysis_struct_fields() {
        let analysis = Analysis {
            model_performance: vec![],
            tool_performance: vec![],
            model_trends: vec![],
            prompt_effectiveness: vec![],
            tool_failure_patterns: vec![],
            common_errors: vec![],
            session_quality: vec![],
            recommendations: vec!["test".to_string()],
            config_optimizations: vec![],
        };
        assert_eq!(analysis.recommendations.len(), 1);
    }
}
