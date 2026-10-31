//! `expect(locator)` — auto-retrying web-first assertions.
//!
//! Each assertion polls the page until the condition holds or the timeout
//! elapses (default 5s), so there is no need to sleep or retry manually. This
//! mirrors `expect(locator)` in the JS/Python bindings. A failed assertion
//! returns an [`Error::Driver`](crate::error::Error::Driver) with name
//! `AssertionError`.

use std::time::{Duration, Instant};

use crate::api::Locator;
use crate::error::{Error, Result};

/// Begins an assertion chain against a [`Locator`].
pub fn expect(locator: &Locator) -> LocatorAssertions {
    LocatorAssertions {
        locator: locator.clone(),
        is_not: false,
        timeout: Duration::from_secs(5),
        poll: Duration::from_millis(100),
    }
}

/// Web-first assertions for a [`Locator`]; see [`expect`].
#[derive(Clone)]
pub struct LocatorAssertions {
    locator: Locator,
    is_not: bool,
    timeout: Duration,
    poll: Duration,
}

impl LocatorAssertions {
    /// Inverts the assertion (e.g. `expect(l).not().to_be_visible()`).
    pub fn not(mut self) -> Self {
        self.is_not = !self.is_not;
        self
    }

    /// Overrides the retry timeout (default 5 seconds).
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn fail(&self, condition: &str, last: &str) -> Error {
        Error::Driver {
            name: "AssertionError".to_string(),
            message: format!(
                "expect({}){} {}; last observed: {}",
                self.locator.selector(),
                if self.is_not { " not" } else { "" },
                condition,
                last,
            ),
            stack: String::new(),
        }
    }

    /// Polls `check` until it returns the expected boolean (accounting for
    /// `not()`), or the timeout elapses.
    async fn poll_bool<F, Fut>(&self, condition: &str, mut check: F) -> Result<()>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = (bool, String)>,
    {
        let want = !self.is_not;
        let deadline = Instant::now() + self.timeout;
        loop {
            let (ok, observed) = check().await;
            if ok == want {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(self.fail(condition, &observed));
            }
            tokio::time::sleep(self.poll).await;
        }
    }

    /// Asserts the element is visible.
    pub async fn to_be_visible(&self) -> Result<()> {
        let l = self.locator.clone();
        self.poll_bool("to be visible", move || {
            let l = l.clone();
            async move {
                let v = l.is_visible().await.unwrap_or(false);
                (v, v.to_string())
            }
        })
        .await
    }

    /// Asserts the element is hidden or absent.
    pub async fn to_be_hidden(&self) -> Result<()> {
        let l = self.locator.clone();
        self.poll_bool("to be hidden", move || {
            let l = l.clone();
            async move {
                let v = l.is_hidden().await.unwrap_or(true);
                (v, format!("hidden={v}"))
            }
        })
        .await
    }

    /// Asserts the element is attached to the DOM (at least one match).
    pub async fn to_be_attached(&self) -> Result<()> {
        let l = self.locator.clone();
        self.poll_bool("to be attached", move || {
            let l = l.clone();
            async move {
                let c = l.count().await.unwrap_or(0);
                (c > 0, format!("count={c}"))
            }
        })
        .await
    }

    /// Asserts the element is enabled.
    pub async fn to_be_enabled(&self) -> Result<()> {
        let l = self.locator.clone();
        self.poll_bool("to be enabled", move || {
            let l = l.clone();
            async move {
                let v = l.is_enabled().await.unwrap_or(false);
                (v, v.to_string())
            }
        })
        .await
    }

    /// Asserts the element is disabled.
    pub async fn to_be_disabled(&self) -> Result<()> {
        let l = self.locator.clone();
        self.poll_bool("to be disabled", move || {
            let l = l.clone();
            async move {
                let v = l.is_disabled().await.unwrap_or(false);
                (v, v.to_string())
            }
        })
        .await
    }

    /// Asserts the element is editable.
    pub async fn to_be_editable(&self) -> Result<()> {
        let l = self.locator.clone();
        self.poll_bool("to be editable", move || {
            let l = l.clone();
            async move {
                let v = l.is_editable().await.unwrap_or(false);
                (v, v.to_string())
            }
        })
        .await
    }

    /// Asserts a checkbox/radio is checked.
    pub async fn to_be_checked(&self) -> Result<()> {
        let l = self.locator.clone();
        self.poll_bool("to be checked", move || {
            let l = l.clone();
            async move {
                let v = l.is_checked().await.unwrap_or(false);
                (v, v.to_string())
            }
        })
        .await
    }

    /// Asserts the element is the active (focused) element.
    pub async fn to_be_focused(&self) -> Result<()> {
        let l = self.locator.clone();
        self.poll_bool("to be focused", move || {
            let l = l.clone();
            async move {
                let v = l
                    .evaluate("el => el === document.activeElement")
                    .await
                    .ok()
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                (v, v.to_string())
            }
        })
        .await
    }

    /// Asserts the element's text equals `text` (whitespace-normalized).
    pub async fn to_have_text(&self, text: &str) -> Result<()> {
        let l = self.locator.clone();
        let want = normalize(text);
        self.poll_bool("to have text", move || {
            let l = l.clone();
            let want = want.clone();
            async move {
                let actual = l.inner_text().await.unwrap_or_default();
                let got = normalize(&actual);
                (got == want, got)
            }
        })
        .await
    }

    /// Asserts the element's text contains `text`.
    pub async fn to_contain_text(&self, text: &str) -> Result<()> {
        let l = self.locator.clone();
        let needle = normalize(text);
        self.poll_bool("to contain text", move || {
            let l = l.clone();
            let needle = needle.clone();
            async move {
                let actual = l.inner_text().await.unwrap_or_default();
                let got = normalize(&actual);
                (got.contains(&needle), got)
            }
        })
        .await
    }

    /// Asserts an input's value equals `value`.
    pub async fn to_have_value(&self, value: &str) -> Result<()> {
        let l = self.locator.clone();
        let want = value.to_string();
        self.poll_bool("to have value", move || {
            let l = l.clone();
            let want = want.clone();
            async move {
                let got = l.input_value().await.unwrap_or_default();
                (got == want, got)
            }
        })
        .await
    }

    /// Asserts the attribute `name` equals `value`.
    pub async fn to_have_attribute(&self, name: &str, value: &str) -> Result<()> {
        let l = self.locator.clone();
        let name = name.to_string();
        let want = value.to_string();
        self.poll_bool("to have attribute", move || {
            let l = l.clone();
            let name = name.clone();
            let want = want.clone();
            async move {
                let got = l.get_attribute(&name).await.ok().flatten();
                (got.as_deref() == Some(want.as_str()), format!("{got:?}"))
            }
        })
        .await
    }

    /// Asserts the number of matching elements equals `count`.
    pub async fn to_have_count(&self, count: i64) -> Result<()> {
        let l = self.locator.clone();
        self.poll_bool("to have count", move || {
            let l = l.clone();
            async move {
                let c = l.count().await.unwrap_or(-1);
                (c == count, c.to_string())
            }
        })
        .await
    }
}

/// Begins an assertion chain against a [`Page`](crate::api::Page).
pub fn expect_page(page: &crate::api::Page) -> PageAssertions {
    PageAssertions {
        page: page.clone(),
        is_not: false,
        timeout: Duration::from_secs(5),
        poll: Duration::from_millis(100),
    }
}

/// Web-first assertions for a [`Page`](crate::api::Page); see [`expect_page`].
#[derive(Clone)]
pub struct PageAssertions {
    page: crate::api::Page,
    is_not: bool,
    timeout: Duration,
    poll: Duration,
}

impl PageAssertions {
    /// Inverts the assertion.
    pub fn not(mut self) -> Self {
        self.is_not = !self.is_not;
        self
    }

    /// Overrides the retry timeout (default 5 seconds).
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    async fn poll_bool<F, Fut>(&self, condition: &str, mut check: F) -> Result<()>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = (bool, String)>,
    {
        let want = !self.is_not;
        let deadline = Instant::now() + self.timeout;
        loop {
            let (ok, observed) = check().await;
            if ok == want {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(Error::Driver {
                    name: "AssertionError".to_string(),
                    message: format!(
                        "expect(page){} {}; last observed: {}",
                        if self.is_not { " not" } else { "" },
                        condition,
                        observed
                    ),
                    stack: String::new(),
                });
            }
            tokio::time::sleep(self.poll).await;
        }
    }

    /// Asserts the page title equals `title` (whitespace-normalized).
    pub async fn to_have_title(&self, title: &str) -> Result<()> {
        let page = self.page.clone();
        let want = normalize(title);
        self.poll_bool("to have title", move || {
            let page = page.clone();
            let want = want.clone();
            async move {
                let got = normalize(&page.title().await.unwrap_or_default());
                (got == want, got)
            }
        })
        .await
    }

    /// Asserts the page URL equals `url`.
    pub async fn to_have_url(&self, url: &str) -> Result<()> {
        let page = self.page.clone();
        let want = url.to_string();
        self.poll_bool("to have url", move || {
            let page = page.clone();
            let want = want.clone();
            async move {
                let got = page.url().await.unwrap_or_default();
                (got == want, got)
            }
        })
        .await
    }
}

/// Begins an assertion chain against an API response value.
pub fn expect_response(response: &crate::protocol::APIResponse) -> ApiResponseAssertions {
    ApiResponseAssertions {
        status: response.status,
        url: response.url.clone(),
        is_not: false,
    }
}

/// Assertions for an [`APIResponse`](crate::protocol::APIResponse). The response
/// is an immutable snapshot, so these do not poll.
#[derive(Clone)]
pub struct ApiResponseAssertions {
    status: i64,
    url: String,
    is_not: bool,
}

impl ApiResponseAssertions {
    /// Inverts the assertion.
    pub fn not(mut self) -> Self {
        self.is_not = !self.is_not;
        self
    }

    /// Asserts the response status is in the 2xx range.
    pub async fn to_be_ok(&self) -> Result<()> {
        let ok = (200..300).contains(&self.status);
        if ok == !self.is_not {
            Ok(())
        } else {
            Err(Error::Driver {
                name: "AssertionError".to_string(),
                message: format!(
                    "expect(response){} to be ok; status {} for {}",
                    if self.is_not { " not" } else { "" },
                    self.status,
                    self.url,
                ),
                stack: String::new(),
            })
        }
    }
}

/// Collapses runs of whitespace and trims, matching Playwright's
/// `normalizeWhiteSpace`.
fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
