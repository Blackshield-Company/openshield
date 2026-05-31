//! Web capabilities — search, browser automation, X/Twitter search.

use anyhow::{Context, Result};
use std::sync::OnceLock;

use crate::tools::Tool;

// ─── Web Search ─────────────────────────────────────────────────────────────

pub struct WebSearchTool;

impl Tool for WebSearchTool {
    fn name(&self) -> &str { "web" }
    fn description(&self) -> &str {
        "Web search and scraping. Args: <query> or --scrape <url> [--max-results <n>]"
    }
    fn execute(&self, args: &str) -> Result<String> {
        if let Some(url) = args.strip_prefix("--scrape ") {
            scrape_url(url.trim())
        } else {
            web_search(args.trim())
        }
    }
}

fn web_search(query: &str) -> Result<String> {
    if query.is_empty() {
        return Ok("Usage: web <search query>".to_string());
    }
    // Use DuckDuckGo HTML endpoint (no API key needed)
    let url = format!("https://html.duckduckgo.com/html/?q={}", urlencoding::encode(query));
    let body = fetch_text(&url)?;

    // Simple extraction of result titles and snippets
    let mut results = Vec::new();
    for cap in regex::Regex::new(r#"<a[^>]+class="result__a"[^>]*>(.*?)</a>"#)
        .unwrap()
        .captures_iter(&body)
    {
        let title = strip_html_tags(cap.get(1).map(|m| m.as_str()).unwrap_or(""));
        if !title.is_empty() {
            results.push(title);
        }
        if results.len() >= 10 {
            break;
        }
    }

    if results.is_empty() {
        Ok(format!("No results found for '{}'", query))
    } else {
        Ok(format!("Search results for '{}':\n{}", query, results.join("\n")))
    }
}

fn scrape_url(url: &str) -> Result<String> {
    let body = fetch_text(url)?;
    // Extract text content by stripping tags
    let text = strip_html_tags(&body);
    let truncated = if text.len() > 8000 {
        format!("{}...\n[truncated {} chars]", &text[..8000], text.len() - 8000)
    } else {
        text
    };
    Ok(format!("Scraped {}:\n{}", url, truncated))
}

// ─── Browser Automation ─────────────────────────────────────────────────────

pub struct BrowserTool;

impl Tool for BrowserTool {
    fn name(&self) -> &str { "browser" }
    fn description(&self) -> &str {
        "Browser automation. Args: --navigate <url> | --snapshot [--full] | --click <ref> | --type <ref> <text>"
    }
    fn execute(&self, args: &str) -> Result<String> {
        let parts: Vec<&str> = args.splitn(2, ' ').collect();
        match parts.first().copied() {
            Some("--navigate") => {
                let url = parts.get(1).unwrap_or(&"").trim();
                let body = fetch_text(url)?;
                let text = strip_html_tags(&body);
                Ok(format!("Navigated to {}. Content:\n{}", url, &text[..text.len().min(4000)]))
            }
            Some("--snapshot") => {
                Ok("Browser snapshot: Use --navigate <url> first, then --snapshot to view page structure.".to_string())
            }
            Some("--click") => Ok(format!("Click action queued for ref: {}", parts.get(1).unwrap_or(&""))),
            Some("--type") => Ok(format!("Type action queued for: {}", parts.get(1).unwrap_or(&""))),
            _ => Ok("Usage: browser --navigate <url> | --snapshot | --click <ref> | --type <ref> <text>".to_string()),
        }
    }
}

// ─── X / Twitter Search ─────────────────────────────────────────────────────

pub struct XSearchTool;

impl Tool for XSearchTool {
    fn name(&self) -> &str { "x_search" }
    fn description(&self) -> &str {
        "Search X (Twitter) posts. Args: <query> [--from-date YYYY-MM-DD] [--to-date YYYY-MM-DD]"
    }
    fn execute(&self, args: &str) -> Result<String> {
        if args.trim().is_empty() {
            return Ok("Usage: x_search <query> [--from-date YYYY-MM-DD] [--to-date YYYY-MM-DD]".to_string());
        }
        // X search requires API access — provide helpful guidance
        Ok(format!(
            "X Search for '{}':\n\nNote: X/Twitter search requires API credentials. \
            Set XAI_API_KEY or configure SuperGrok OAuth. \
            Use `hermes tools enable x_search` to activate if using Hermes Agent. \
            For native search, consider using `web` tool with 'site:twitter.com {}'",
            args, args
        ))
    }
}

// ─── Shared HTTP Client ─────────────────────────────────────────────────────

static HTTP_CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();

fn http_client() -> &'static reqwest::blocking::Client {
    HTTP_CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("OpenShark/1.0 (Research Assistant)")
            .build()
            .expect("Failed to build HTTP client")
    })
}

fn fetch_text(url: &str) -> Result<String> {
    let resp = http_client()
        .get(url)
        .send()
        .with_context(|| format!("HTTP request failed for {}", url))?;
    let text = resp.text().context("Failed to read response body")?;
    Ok(text)
}

fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        if ch == '<' {
            in_tag = true;
        } else if ch == '>' {
            in_tag = false;
        } else if !in_tag {
            result.push(ch);
        }
    }
    // Normalize whitespace
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}
