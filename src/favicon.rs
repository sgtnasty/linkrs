//! Fetching and storing each link's site icon (favicon).
//!
//! Icons are fetched in the background after a link is created (or its URL
//! changes), and once at startup for any link that has never been checked,
//! so adding a link never waits on a slow or unreachable site. The result is
//! stored in the `link_icons` table — including "checked, none found", so
//! icon-less sites aren't re-fetched on every restart. A fetch that fails
//! on a network error records nothing, leaving the link to be retried by
//! the next startup's backfill.
//!
//! Only raster formats recognized by their magic bytes are accepted (see
//! [`sniff_image_type`]); the server's `Content-Type` header is ignored, and
//! SVG is always refused, since icons are served back from the app's own
//! origin and an SVG can carry script.
//!
//! These are outbound requests to whatever URL a logged-in user saves,
//! including internal addresses — acceptable under the trusted/self-hosted
//! threat model described in the README's "Authentication" section.

use std::time::Duration;

use reqwest::{redirect, Client, Response, Url};

use crate::db;
use crate::state::AppState;

/// Largest icon body accepted, in bytes. Real favicons are a few KB; this
/// mostly guards against a misconfigured server returning something huge.
const MAX_ICON_BYTES: usize = 100 * 1024;

/// How much of a page's HTML is scanned for `<link rel="icon">` tags. They
/// belong in `<head>`, so the start of the document is enough.
const MAX_HTML_BYTES: usize = 256 * 1024;

/// Builds the HTTP client used for all icon fetches: short timeout, a
/// bounded redirect chain, and an honest user agent.
///
/// # Panics
///
/// Panics if the client can't be built (only on TLS backend init failure).
pub fn build_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(5))
        .redirect(redirect::Policy::limited(5))
        .user_agent(concat!("linkrs/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("failed to build HTTP client")
}

/// Fetches the icon for `link_id` in a background task and stores the
/// result. The store is skipped if the link was deleted or its URL changed
/// while the fetch was in flight (see [`db::set_link_icon`]).
pub fn spawn_fetch(state: AppState, link_id: i64, url: String) {
    tokio::spawn(async move {
        fetch_and_store(&state, link_id, &url).await;
    });
}

/// Fetches icons, one at a time, for every link that has never been
/// checked — links added before this feature existed, or whose last fetch
/// hit a network error or was cut short by a restart.
pub fn spawn_backfill(state: AppState) {
    tokio::spawn(async move {
        let pending = {
            let conn = state.db.lock().unwrap();
            db::links_missing_icon_check(&conn)
        };
        match pending {
            Ok(links) => {
                for (id, url) in links {
                    fetch_and_store(&state, id, &url).await;
                }
            }
            Err(e) => tracing::warn!("favicon backfill: failed to list links: {e}"),
        }
    });
}

async fn fetch_and_store(state: &AppState, link_id: i64, url: &str) {
    let icon = match fetch_icon(&state.http, url).await {
        Ok(icon) => icon,
        Err(e) => {
            // Leave the link unchecked so the next startup's backfill retries.
            tracing::debug!("favicon for link {link_id}: fetch failed, will retry: {e}");
            return;
        }
    };
    match &icon {
        Some((content_type, data)) => {
            tracing::debug!(
                "favicon for link {link_id}: {content_type}, {} bytes",
                data.len()
            )
        }
        None => tracing::debug!("favicon for link {link_id}: none found"),
    }
    let conn = state.db.lock().unwrap();
    let icon_ref = icon.as_ref().map(|(ct, data)| (*ct, data.as_slice()));
    if let Err(e) = db::set_link_icon(&conn, link_id, url, icon_ref) {
        tracing::warn!("favicon for link {link_id}: failed to store: {e}");
    }
}

/// A found icon: its sniffed content type and bytes.
type Icon = (&'static str, Vec<u8>);

/// Finds an icon for the site at `page_url`: first `<origin>/favicon.ico`,
/// then any `<link rel="icon">` (or `apple-touch-icon`) the page declares.
///
/// `Ok(None)` means the site answered but has no usable icon (or the URL
/// isn't http(s)), and is worth remembering; `Err` means fetching the page
/// itself hit a transport error (DNS, connect, timeout) that may well be
/// transient. A failure fetching one candidate icon just moves on to the
/// next, so a stalled `/favicon.ico` doesn't block the page's declared
/// icons.
async fn fetch_icon(client: &Client, page_url: &str) -> reqwest::Result<Option<Icon>> {
    let Ok(page_url) = Url::parse(page_url) else {
        return Ok(None);
    };
    if !matches!(page_url.scheme(), "http" | "https") {
        return Ok(None);
    }

    if let Ok(favicon_url) = page_url.join("/favicon.ico") {
        if let Ok(Some(icon)) = fetch_image(client, favicon_url).await {
            return Ok(Some(icon));
        }
    }

    let resp = client.get(page_url).send().await?;
    if !resp.status().is_success() || !is_html(&resp) {
        return Ok(None);
    }
    // Resolve relative hrefs against where any redirects actually landed.
    let base = resp.url().clone();
    let Some(html) = read_body(resp, MAX_HTML_BYTES, true).await? else {
        return Ok(None);
    };
    for href in find_icon_hrefs(&String::from_utf8_lossy(&html)) {
        let Ok(icon_url) = base.join(&href) else {
            continue;
        };
        if let Ok(Some(icon)) = fetch_image(client, icon_url).await {
            return Ok(Some(icon));
        }
    }
    Ok(None)
}

async fn fetch_image(client: &Client, url: Url) -> reqwest::Result<Option<Icon>> {
    if !matches!(url.scheme(), "http" | "https") {
        return Ok(None);
    }
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Ok(None);
    }
    let data = read_body(resp, MAX_ICON_BYTES, false).await?;
    Ok(data.and_then(|data| Some((sniff_image_type(&data)?, data))))
}

fn is_html(resp: &Response) -> bool {
    resp.headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.to_ascii_lowercase().starts_with("text/html"))
}

/// Reads a response body up to `cap` bytes. Past the cap, returns what was
/// read so far if `truncate` is set, or `None` otherwise.
async fn read_body(
    mut resp: Response,
    cap: usize,
    truncate: bool,
) -> reqwest::Result<Option<Vec<u8>>> {
    if !truncate && resp.content_length().is_some_and(|len| len > cap as u64) {
        return Ok(None);
    }
    let mut buf = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        if buf.len() + chunk.len() > cap {
            if !truncate {
                return Ok(None);
            }
            buf.extend_from_slice(&chunk[..cap - buf.len()]);
            break;
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(Some(buf))
}

/// Identifies a raster image format from its leading magic bytes, returning
/// the MIME type to serve it as. Anything unrecognized — notably SVG, or an
/// HTML error page served with a `200` — returns `None`.
fn sniff_image_type(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if data.starts_with(&[0, 0, 1, 0]) {
        Some("image/x-icon")
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        Some("image/webp")
    } else if data.starts_with(b"BM") {
        Some("image/bmp")
    } else {
        None
    }
}

/// Extracts icon `href`s from `<link>` tags in `html`, `rel="icon"` (incl.
/// `"shortcut icon"`) ones before `apple-touch-icon` ones. SVG-declared icons
/// are skipped up front (they'd be rejected by [`sniff_image_type`] anyway).
///
/// A deliberately small scanner rather than a full HTML parser: it only has
/// to cope with the attribute syntax real `<link>` tags use.
fn find_icon_hrefs(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut icons = Vec::new();
    let mut touch_icons = Vec::new();
    let mut pos = 0;
    while let Some(start) = lower[pos..].find("<link") {
        let start = pos + start;
        let Some(len) = lower[start..].find('>') else {
            break;
        };
        let end = start + len;
        pos = end;

        let attrs = parse_attrs(&html[start + "<link".len()..end]);
        let get = |name: &str| {
            attrs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.as_str())
        };
        let (Some(rel), Some(href)) = (get("rel"), get("href")) else {
            continue;
        };
        let href = href.trim().replace("&amp;", "&");
        let is_svg = get("type").is_some_and(|t| t.eq_ignore_ascii_case("image/svg+xml"))
            || href
                .to_ascii_lowercase()
                .split(['?', '#'])
                .next()
                .unwrap_or("")
                .ends_with(".svg");
        if href.is_empty() || is_svg {
            continue;
        }
        let rel = rel.to_ascii_lowercase();
        let mut tokens = rel.split_ascii_whitespace();
        if tokens.clone().any(|t| t == "icon") {
            icons.push(href);
        } else if tokens.any(|t| t == "apple-touch-icon" || t == "apple-touch-icon-precomposed") {
            touch_icons.push(href);
        }
    }
    icons.extend(touch_icons);
    icons
}

/// Parses `name=value` / `name="value"` / `name='value'` attributes out of
/// the inside of a tag. Names are lowercased; bare attributes get an empty
/// value.
fn parse_attrs(s: &str) -> Vec<(String, String)> {
    let mut attrs = Vec::new();
    let mut chars = s.char_indices().peekable();
    loop {
        while chars
            .next_if(|(_, c)| c.is_whitespace() || *c == '/')
            .is_some()
        {}
        let Some(&(name_start, _)) = chars.peek() else {
            break;
        };
        while chars
            .next_if(|(_, c)| !c.is_whitespace() && *c != '=')
            .is_some()
        {}
        let name_end = chars.peek().map_or(s.len(), |&(i, _)| i);
        let name = s[name_start..name_end].to_ascii_lowercase();
        while chars.next_if(|(_, c)| c.is_whitespace()).is_some() {}
        if chars.next_if(|(_, c)| *c == '=').is_none() {
            attrs.push((name, String::new()));
            continue;
        }
        while chars.next_if(|(_, c)| c.is_whitespace()).is_some() {}
        let value = match chars.peek() {
            Some(&(i, q @ ('"' | '\''))) => {
                chars.next();
                let mut end = s.len();
                for (j, c) in chars.by_ref() {
                    if c == q {
                        end = j;
                        break;
                    }
                }
                &s[i + 1..end]
            }
            Some(&(i, _)) => {
                while chars.next_if(|(_, c)| !c.is_whitespace()).is_some() {}
                &s[i..chars.peek().map_or(s.len(), |&(j, _)| j)]
            }
            None => "",
        };
        attrs.push((name, value.to_string()));
    }
    attrs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_icons_in_priority_order() {
        let html = r#"<head>
            <link rel="apple-touch-icon" href="/touch.png">
            <link rel="stylesheet" href="/style.css">
            <LINK REL='Shortcut Icon' HREF='/a.ico?v=1&amp;x=2'>
            <link href=/b.png rel=icon sizes=32x32 />
            <link rel="icon" type="image/svg+xml" href="/c">
            <link rel="icon" href="/d.svg?v=3">
            <link rel="mask-icon" href="/e.png">
        </head>"#;
        assert_eq!(
            find_icon_hrefs(html),
            vec!["/a.ico?v=1&x=2", "/b.png", "/touch.png"]
        );
    }

    #[test]
    fn sniffs_raster_formats_and_rejects_svg() {
        assert_eq!(
            sniff_image_type(b"\x89PNG\r\n\x1a\n...."),
            Some("image/png")
        );
        assert_eq!(sniff_image_type(&[0, 0, 1, 0, 1, 0]), Some("image/x-icon"));
        assert_eq!(
            sniff_image_type(b"RIFF\0\0\0\0WEBPVP8 "),
            Some("image/webp")
        );
        assert_eq!(
            sniff_image_type(b"<svg xmlns=\"http://www.w3.org/2000/svg\">"),
            None
        );
        assert_eq!(sniff_image_type(b"<?xml version=\"1.0\"?><svg>"), None);
        assert_eq!(sniff_image_type(b"<!DOCTYPE html>"), None);
    }
}
