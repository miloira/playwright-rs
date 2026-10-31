//! Network interception: `page.route(glob, handler)` /
//! `context.route(glob, handler)` plus [`Route`] response helpers.
//!
//! Registering a route tells the driver to intercept matching requests and
//! emit a `route` event carrying a [`Route`]. A single dispatcher per
//! page/context fans those events out to the registered handlers; if no handler
//! matches a request, it is continued automatically so nothing stalls.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::json;

use crate::api::{BrowserContext, Page, Request, Route};
use crate::connection::Connection;
use crate::error::Result;

type BoxFuture = Pin<Box<dyn Future<Output = ()> + Send>>;
type Handler = Arc<dyn Fn(Route) -> BoxFuture + Send + Sync>;

struct Entry {
    glob: String,
    handler: Handler,
}

#[derive(Default)]
struct RouteState {
    entries: Vec<Entry>,
    dispatcher_started: bool,
}

fn registry() -> &'static Mutex<HashMap<String, RouteState>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, RouteState>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

impl Route {
    /// Continues the request unmodified.
    pub async fn continue_(&self) -> Result<()> {
        self.connection()
            .send(&self.guid, "continue", json!({ "isFallback": false }))
            .await
            .map(drop)
    }

    /// Aborts the request with an optional error code (e.g. `"failed"`).
    ///
    /// (The generated [`Route::abort`] takes the full protocol options; this is
    /// the convenient shortcut.)
    pub async fn abort_now(&self, error_code: Option<&str>) -> Result<()> {
        let params = match error_code {
            Some(code) => json!({ "errorCode": code }),
            None => json!({}),
        };
        self.connection().send(&self.guid, "abort", params).await.map(drop)
    }

    /// Fulfills the request with a synthetic text response.
    ///
    /// (The generated [`Route::fulfill`] takes the full protocol options; this
    /// is the convenient shortcut.)
    pub async fn fulfill_text(&self, status: u16, content_type: &str, body: &str) -> Result<()> {
        self.connection()
            .send(
                &self.guid,
                "fulfill",
                json!({
                    "status": status,
                    "headers": [{ "name": "content-type", "value": content_type }],
                    "body": body,
                    "isBase64": false,
                }),
            )
            .await
            .map(drop)
    }

    /// The request being routed.
    pub async fn request(&self) -> Result<Request> {
        let init = self.initializer().await?;
        Ok(Request::from_channel(self.connection(), init.request.guid))
    }

    /// The URL of the request being routed.
    pub async fn url(&self) -> Result<String> {
        let init = self.initializer().await?;
        let req = self.connection().initializer(&init.request.guid).await?;
        Ok(req.get("url").and_then(|v| v.as_str()).unwrap_or_default().to_string())
    }
}

impl Page {
    /// Intercepts requests whose URL matches the glob, invoking `handler` with a
    /// [`Route`] (call `continue_`, `fulfill` or `abort` on it).
    ///
    /// `handler` may be called concurrently for different requests.
    pub async fn route<F, Fut>(&self, glob: &str, handler: F) -> Result<()>
    where
        F: Fn(Route) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        register_route(self.connection(), self.guid.clone(), glob, handler).await
    }
}

impl Page {
    /// Removes routes registered with [`Page::route`]. Pass the same `glob` to
    /// remove one pattern, or `None` to remove all.
    pub async fn unroute(&self, glob: Option<&str>) -> Result<()> {
        unregister_route(self.connection(), self.guid.clone(), glob).await
    }
}

impl BrowserContext {
    /// Like [`Page::route`], but for every page in the context.
    pub async fn route<F, Fut>(&self, glob: &str, handler: F) -> Result<()>
    where
        F: Fn(Route) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        register_route(self.connection(), self.guid.clone(), glob, handler).await
    }

    /// Removes routes registered with [`BrowserContext::route`].
    pub async fn unroute(&self, glob: Option<&str>) -> Result<()> {
        unregister_route(self.connection(), self.guid.clone(), glob).await
    }
}

async fn unregister_route(conn: Connection, guid: String, glob: Option<&str>) -> Result<()> {
    let patterns = {
        let mut reg = registry().lock().unwrap();
        if let Some(state) = reg.get_mut(&guid) {
            match glob {
                Some(g) => state.entries.retain(|e| e.glob != g),
                None => state.entries.clear(),
            }
            state.entries.iter().map(|e| json!({ "glob": e.glob })).collect::<Vec<_>>()
        } else {
            Vec::new()
        }
    };
    conn.send(&guid, "setNetworkInterceptionPatterns", json!({ "patterns": patterns }))
        .await?;
    Ok(())
}

async fn register_route<F, Fut>(conn: Connection, guid: String, glob: &str, handler: F) -> Result<()>
where
    F: Fn(Route) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let boxed: Handler = Arc::new(move |route| Box::pin(handler(route)) as BoxFuture);

    let (patterns, need_dispatcher) = {
        let mut reg = registry().lock().unwrap();
        let state = reg.entry(guid.clone()).or_default();
        state.entries.push(Entry { glob: glob.to_string(), handler: boxed });
        let patterns: Vec<_> = state.entries.iter().map(|e| json!({ "glob": e.glob })).collect();
        let need = !state.dispatcher_started;
        state.dispatcher_started = true;
        (patterns, need)
    };

    // Subscribe before enabling interception so no route event is missed.
    let events = if need_dispatcher {
        Some(conn.subscribe(&guid).await)
    } else {
        None
    };

    conn.send(&guid, "setNetworkInterceptionPatterns", json!({ "patterns": patterns }))
        .await?;

    if let Some(mut events) = events {
        let dconn = conn.clone();
        let dguid = guid.clone();
        tokio::spawn(async move {
            while let Some((method, params)) = events.recv().await {
                if method != "route" {
                    continue;
                }
                let route_guid = match params.get("route").and_then(|r| r.get("guid")).and_then(|v| v.as_str()) {
                    Some(g) => g.to_string(),
                    None => continue,
                };
                let route = Route::from_channel(dconn.clone(), route_guid);
                let url = route.url().await.unwrap_or_default();

                let handler = {
                    let reg = registry().lock().unwrap();
                    reg.get(&dguid).and_then(|s| {
                        s.entries
                            .iter()
                            .rev()
                            .find(|e| glob_match(&e.glob, &url))
                            .map(|e| e.handler.clone())
                    })
                };

                match handler {
                    Some(h) => h(route).await,
                    None => {
                        let _ = route.continue_().await;
                    }
                }
            }
        });
    }

    Ok(())
}

/// A small glob matcher: `*` and `**` match any run of characters, `?` matches
/// a single character, everything else is literal.
fn glob_match(glob: &str, text: &str) -> bool {
    let p: Vec<char> = glob.replace("**", "*").chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (None, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::glob_match;

    #[test]
    fn globs() {
        assert!(glob_match("**/*", "https://x.test/a/b.png"));
        assert!(glob_match("*.png", "logo.png"));
        assert!(!glob_match("*.png", "logo.css"));
        assert!(glob_match("https://x.test/*", "https://x.test/anything"));
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "ac"));
    }
}
