use anyhow::{Context, Result};
use async_trait::async_trait;

use super::Tool;

// ── web_fetch ─────────────────────────────────────────────────────────────────

pub(super) struct WebFetchTool;

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str { "web_fetch" }
    fn description(&self) -> &str {
        "Fetch a URL and return its content as plain text (HTML tags stripped). \
         Useful for reading documentation, API references, or web pages."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url":       { "type": "string",  "description": "URL to fetch." },
                "max_chars": { "type": "integer", "description": "Maximum characters to return (default 8000)." }
            },
            "required": ["url"]
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        format!("fetch '{}'", input["url"].as_str().unwrap_or("?"))
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let url       = input["url"].as_str().context("web_fetch: 'url' required")?;
        let max_chars = input["max_chars"].as_u64().unwrap_or(8000) as usize;

        let client = crate::http::client();

        let resp = client.get(url).send().await
            .with_context(|| format!("web_fetch: could not reach '{}'", url))?;

        let status = resp.status();
        if !status.is_success() {
            anyhow::bail!("web_fetch: HTTP {} from '{}'", status, url);
        }

        let body = resp.text().await?;
        let text = strip_html(&body);

        if text.len() > max_chars {
            Ok(format!("{}\n\n[…truncated to {} chars of {}]",
                &text[..max_chars], max_chars, text.len()))
        } else {
            Ok(text)
        }
    }
}

// ── web_search ────────────────────────────────────────────────────────────────

pub(super) struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str { "web_search" }
    fn description(&self) -> &str {
        "Search the web using DuckDuckGo and return top results with titles and URLs. \
         No API key required."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query":       { "type": "string",  "description": "Search query." },
                "max_results": { "type": "integer", "description": "Max results to return (default 5)." }
            },
            "required": ["query"]
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        format!("search: '{}'", input["query"].as_str().unwrap_or("?"))
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let query = input["query"].as_str().context("web_search: 'query' required")?;
        let max   = input["max_results"].as_u64().unwrap_or(5) as usize;

        let client = crate::http::client();

        let resp = client
            .get("https://api.duckduckgo.com/")
            .query(&[
                ("q", query),
                ("format", "json"),
                ("no_html", "1"),
                ("skip_disambig", "1"),
            ])
            .send()
            .await
            .context("web_search: could not reach DuckDuckGo")?;

        let json: serde_json::Value = resp.json().await
            .context("web_search: could not parse response")?;

        let mut results: Vec<String> = Vec::new();

        if let Some(text) = json["AbstractText"].as_str().filter(|s| !s.is_empty()) {
            let url = json["AbstractURL"].as_str().unwrap_or("");
            results.push(format!("[Summary]\n{}\n{}", text, url));
        }

        if let Some(arr) = json["Results"].as_array() {
            for item in arr.iter().take(max) {
                let title = item["Text"].as_str().unwrap_or("?");
                let url   = item["FirstURL"].as_str().unwrap_or("");
                results.push(format!("• {}\n  {}", title, url));
            }
        }

        if let Some(arr) = json["RelatedTopics"].as_array() {
            for item in arr.iter() {
                if results.len() >= max + 1 { break; }
                if item["Topics"].is_array() { continue; }
                let text = item["Text"].as_str().unwrap_or("?");
                let url  = item["FirstURL"].as_str().unwrap_or("");
                if !text.is_empty() {
                    results.push(format!("• {}\n  {}", text, url));
                }
            }
        }

        if results.is_empty() {
            Ok(format!("No results found for '{}'.", query))
        } else {
            Ok(format!("Search results for '{}':\n\n{}", query, results.join("\n\n")))
        }
    }
}

// ── HTML stripping helper ─────────────────────────────────────────────────────

fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut skip_block = false;
    let mut tag_buf = String::new();

    for ch in html.chars() {
        match ch {
            '<' => {
                in_tag = true;
                tag_buf.clear();
            }
            '>' => {
                let tag_lower = tag_buf.to_lowercase();
                if tag_lower.starts_with("script") || tag_lower.starts_with("style") {
                    skip_block = true;
                } else if tag_lower.starts_with("/script") || tag_lower.starts_with("/style") {
                    skip_block = false;
                }
                in_tag = false;
                if !skip_block && !out.ends_with(' ') && !out.ends_with('\n') {
                    out.push(' ');
                }
            }
            _ if in_tag => tag_buf.push(ch),
            _ if !skip_block => out.push(ch),
            _ => {}
        }
    }

    let out = out
        .replace("&amp;",  "&")
        .replace("&lt;",   "<")
        .replace("&gt;",   ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;",  "'")
        .replace("&nbsp;", " ");

    let mut result = String::with_capacity(out.len());
    let mut prev_newline = false;
    for line in out.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !prev_newline { result.push('\n'); }
            prev_newline = true;
        } else {
            result.push_str(trimmed);
            result.push('\n');
            prev_newline = false;
        }
    }
    result.trim().to_string()
}
