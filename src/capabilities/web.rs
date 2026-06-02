//! Web capabilities — search, browser automation, X/Twitter search.

use anyhow::{Context, Result};
use std::sync::{Mutex, OnceLock};

use crate::tools::Tool;
use serde_json::json;
use tungstenite::{Message as WsMsg, connect};

// ─── Web Search ─────────────────────────────────────────────────────────────

pub struct WebSearchTool;

impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web"
    }
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

pub fn web_search(query: &str) -> Result<String> {
    if query.is_empty() {
        return Ok("Usage: web <search query>".to_string());
    }

    // DuckDuckGo HTML — returns real results without JS rendering or captcha.
    match duckduckgo_search(query) {
        Ok(results) if !results.starts_with("No results") => return Ok(results),
        Ok(_) => {}
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Search failed: {}. Try browser --navigate to search directly.",
                e
            ));
        }
    }

    Err(anyhow::anyhow!(
        "No results found. Try a different query or use browser --navigate to search directly."
    ))
}

/// Search via DuckDuckGo HTML interface — no JS, no captcha, clean results.
fn duckduckgo_search(query: &str) -> Result<String> {
    let url = format!(
        "https://html.duckduckgo.com/html/?q={}",
        urlencoding::encode(query)
    );
    let body = fetch_text(&url)?;
    parse_duckduckgo_results(query, &body)
}

/// Parse DuckDuckGo HTML results.
/// Results are in `.result` divs with `.result__a` (title+link) and `.result__snippet` (description).
fn parse_duckduckgo_results(query: &str, body: &str) -> Result<String> {
    use regex::Regex;

    // Extract title and link from result__a
    let title_re = Regex::new(
        r#"class="result__a" href="([^"]+)"[^>]*>([^<]+)"#,
    )
    .unwrap();

    // Extract snippet from result__snippet
    let snippet_re = Regex::new(
        r#"class="result__snippet"[^>]*>([^<]+)"#,
    )
    .unwrap();

    let titles: Vec<(String, String)> = title_re
        .captures_iter(body)
        .filter_map(|cap| {
            let href = cap.get(1)?.as_str();
            let title = strip_html_tags(cap.get(2)?.as_str());
            // DuckDuckGo redirects through their own URL — extract the real URL
            let real_url = extract_ddg_url(href);
            Some((title.trim().to_string(), real_url))
        })
        .filter(|(t, _)| !t.is_empty() && t.len() > 3)
        .take(10)
        .collect();

    let snippets: Vec<String> = snippet_re
        .captures_iter(body)
        .map(|cap| {
            let s = strip_html_tags(cap.get(1).map(|m| m.as_str()).unwrap_or(""));
            if s.len() > 300 {
                s[..300].to_string()
            } else {
                s
            }
        })
        .take(10)
        .collect();

    if titles.is_empty() {
        // Check if we got a "no results" page or were blocked
        if body.contains("No results") || body.contains("no results") {
            return Ok(format!("No results found for '{}'", query));
        }
        if body.contains("captcha") || body.contains("CAPTCHA") {
            return Err(anyhow::anyhow!(
                "DuckDuckGo served a captcha. Try again later or use browser --navigate."
            ));
        }
        return Ok(format!("No results found for '{}'", query));
    }

    let max = titles.len().min(10);
    let mut lines = vec![format!("Search results for '{}':", query)];
    for i in 0..max {
        lines.push(format!("{}. {}\n   {}", i + 1, titles[i].0, titles[i].1));
        if let Some(snippet) = snippets.get(i) {
            if !snippet.is_empty() {
                lines.push(format!("   {}", snippet));
            }
        }
    }

    Ok(lines.join("\n"))
}

/// Extract the real URL from DuckDuckGo's redirect URL.
fn extract_ddg_url(href: &str) -> String {
    // href looks like: //duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com&amp;rut=...
    if let Some(start) = href.find("uddg=") {
        let after = &href[start + 5..];
        let end = after.find('&').unwrap_or(after.len());
        let encoded = &after[..end];
        return urlencoding::decode(encoded)
            .unwrap_or(std::borrow::Cow::Borrowed(encoded))
            .to_string();
    }
    href.to_string()
}

fn scrape_url(url: &str) -> Result<String> {
    let body = fetch_text(url)?;
    let text = strip_html_tags(&body);
    let truncated = if text.len() > 8000 {
        format!(
            "{}...\n[truncated {} chars]",
            &text[..8000],
            text.len() - 8000
        )
    } else {
        text
    };
    Ok(format!("Scraped {}:\n{}", url, truncated))
}

// ─── Browser Automation (CDP via headless Chromium) ────────────────────────

static CDP_SESSION: Mutex<Option<CdpSession>> = Mutex::new(None);

struct CdpSession {
    ws: tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
    cmd_id: u64,
}

impl CdpSession {
    fn send_cmd(&mut self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        self.cmd_id += 1;
        let id = self.cmd_id;
        let msg = json!({"id": id, "method": method, "params": params});
        self.ws
            .send(WsMsg::Text(msg.to_string().into()))
            .context("CDP send failed")?;
        loop {
            let resp = self.ws.read().context("CDP read failed")?;
            if let WsMsg::Text(text) = resp {
                let v: serde_json::Value =
                    serde_json::from_str(&text).context("CDP parse failed")?;
                if let Some(msg_id) = v.get("id").and_then(|i| i.as_u64()) {
                    if msg_id == id {
                        return Ok(v);
                    }
                }
            }
        }
    }
}

fn get_cdp_session() -> Result<std::sync::MutexGuard<'static, Option<CdpSession>>> {
    let mut guard = CDP_SESSION.lock().unwrap();
    if guard.is_none() {
        let port = 9222u16 + (std::process::id() % 100) as u16;
        let _child = std::process::Command::new(chromium_path())
            .args([
                "--headless",
                "--disable-gpu",
                "--no-sandbox",
                &format!("--remote-debugging-port={}", port),
                "about:blank",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("Failed to start Chromium")?;

        std::thread::sleep(std::time::Duration::from_millis(800));
        let json_url = format!("http://localhost:{}/json", port);
        let resp = http_client()
            .get(&json_url)
            .send()
            .context("Failed to connect to Chrome DevTools")?;
        let body = resp.text()?;
        let pages: Vec<serde_json::Value> = serde_json::from_str(&body)?;
        let ws_url = pages
            .first()
            .and_then(|p| p["webSocketDebuggerUrl"].as_str())
            .ok_or_else(|| anyhow::anyhow!("No debuggable page found"))?;

        let (ws, _) = connect(ws_url).context("CDP WebSocket connection failed")?;
        *guard = Some(CdpSession { ws, cmd_id: 0 });
    }
    Ok(guard)
}

fn chromium_path() -> &'static str {
    if std::path::Path::new("/usr/bin/chromium").exists() {
        "/usr/bin/chromium"
    } else if std::path::Path::new("/usr/bin/google-chrome").exists() {
        "/usr/bin/google-chrome"
    } else {
        "chromium"
    }
}

pub struct BrowserTool;

impl Tool for BrowserTool {
    fn name(&self) -> &str {
        "browser"
    }
    fn description(&self) -> &str {
        "Headless browser (Chromium CDP). Args: --navigate <url> | --snapshot [url] | --click <selector> | --type <selector> <text>"
    }
    fn execute(&self, args: &str) -> Result<String> {
        let parts: Vec<&str> = args.splitn(2, ' ').collect();
        let cmd = parts.first().copied().unwrap_or("");
        let rest = parts.get(1).unwrap_or(&"").trim();
        match cmd {
            "--navigate" => browser_navigate(rest),
            "--snapshot" => browser_snapshot(rest),
            "--screenshot" => browser_snapshot(rest),
            "--click" => browser_click(rest),
            "--type" => browser_type(rest),
            _ => Ok(format!(
                "Usage: browser --navigate <url> | --snapshot [url] | --click <selector> | --type <selector> <text>\n\nChrome CDP: {}",
                chromium_path()
            )),
        }
    }
}

fn browser_navigate(url: &str) -> Result<String> {
    if url.is_empty() {
        return Err(anyhow::anyhow!("Usage: browser --navigate <url>"));
    }
    let mut guard = get_cdp_session()?;
    let s = guard
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("No CDP session"))?;

    s.send_cmd("Page.enable", json!({}))?;
    s.send_cmd("Page.navigate", json!({"url": url}))?;
    std::thread::sleep(std::time::Duration::from_millis(2000));

    let result = s.send_cmd("Runtime.evaluate", json!({
        "expression": "document.body ? document.body.innerText : document.documentElement.innerText",
        "returnByValue": true
    }))?;

    let text = result["result"]["result"]["value"]
        .as_str()
        .unwrap_or("(no text content)");

    let truncated = if text.len() > 6000 {
        format!(
            "{}...\n[truncated {} chars]",
            &text[..6000],
            text.len() - 6000
        )
    } else {
        text.to_string()
    };
    Ok(format!("Navigated to {}\n{}", url, truncated))
}

fn browser_snapshot(url_or_empty: &str) -> Result<String> {
    let mut guard = get_cdp_session()?;
    let s = guard
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("No CDP session"))?;

    if !url_or_empty.is_empty() {
        s.send_cmd("Page.navigate", json!({"url": url_or_empty}))?;
        std::thread::sleep(std::time::Duration::from_millis(2000));
    }

    let result = s.send_cmd(
        "Page.captureScreenshot",
        json!({
            "format": "png",
            "clip": {"x": 0, "y": 0, "width": 1280, "height": 900, "scale": 1}
        }),
    )?;

    let b64 = result["result"]["data"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No screenshot data"))?;

    let data_url = format!("data:image/png;base64,{}", b64);
    let size_kb = (b64.len() * 3 / 4) / 1024;
    Ok(format!(
        "Screenshot captured (~{}KB, 1280x900).\n\n--- VISION DATA URL ---\n{}\n\nInclude this data URL as an image attachment for vision analysis.",
        size_kb, data_url
    ))
}

fn browser_click(selector: &str) -> Result<String> {
    if selector.is_empty() {
        return Err(anyhow::anyhow!("Usage: browser --click <css_selector>"));
    }
    let mut guard = get_cdp_session()?;
    let s = guard
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("No CDP session"))?;

    // Escape single quotes and backslashes for JS string
    let escaped = selector.replace('\\', "\\\\").replace('\'', "\\'");
    let js = format!(
        "(() => {{ const el = document.querySelector('{}'); if (el) {{ el.click(); return 'clicked'; }} return 'not found'; }})()",
        escaped
    );
    let result = s.send_cmd(
        "Runtime.evaluate",
        json!({
            "expression": js,
            "returnByValue": true
        }),
    )?;

    let status = result["result"]["result"]["value"]
        .as_str()
        .unwrap_or("unknown");

    std::thread::sleep(std::time::Duration::from_millis(500));
    Ok(format!(
        "Clicked '{}': {}. Use --snapshot to see the result.",
        selector, status
    ))
}

fn browser_type(args: &str) -> Result<String> {
    let parts: Vec<&str> = args.splitn(2, ' ').collect();
    let selector = parts.first().unwrap_or(&"").trim();
    let text = parts.get(1).unwrap_or(&"").trim();
    if selector.is_empty() {
        return Err(anyhow::anyhow!(
            "Usage: browser --type <css_selector> <text>"
        ));
    }
    let mut guard = get_cdp_session()?;
    let s = guard
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("No CDP session"))?;

    let escaped_sel = selector.replace('\\', "\\\\").replace('\'', "\\'");
    let escaped_text = text.replace('\\', "\\\\").replace('\'', "\\'");
    let js = format!(
        "(() => {{ const el = document.querySelector('{}'); if (!el) return 'not found'; el.focus(); el.value = '{}'; el.dispatchEvent(new Event('input', {{ bubbles: true }})); return 'typed'; }})()",
        escaped_sel, escaped_text
    );
    let result = s.send_cmd(
        "Runtime.evaluate",
        json!({
            "expression": js,
            "returnByValue": true
        }),
    )?;

    let status = result["result"]["result"]["value"]
        .as_str()
        .unwrap_or("unknown");

    Ok(format!(
        "Typed '{}' into '{}': {}. Use --snapshot to see the result.",
        text, selector, status
    ))
}

// ─── X / Twitter Search ─────────────────────────────────────────────────────

pub struct XSearchTool;

impl Tool for XSearchTool {
    fn name(&self) -> &str {
        "x_search"
    }
    fn description(&self) -> &str {
        "Search X (Twitter) posts. Args: <query> [--from-date YYYY-MM-DD] [--to-date YYYY-MM-DD]"
    }
    fn execute(&self, args: &str) -> Result<String> {
        if args.trim().is_empty() {
            return Ok(
                "Usage: x_search <query> [--from-date YYYY-MM-DD] [--to-date YYYY-MM-DD]"
                    .to_string(),
            );
        }
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
            .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36")
            .default_headers({
                let mut headers = reqwest::header::HeaderMap::new();
                headers.insert(
                    reqwest::header::ACCEPT,
                    reqwest::header::HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
                );
                headers.insert(
                    reqwest::header::ACCEPT_LANGUAGE,
                    reqwest::header::HeaderValue::from_static("en-US,en;q=0.9"),
                );
                headers
            })
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
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}
