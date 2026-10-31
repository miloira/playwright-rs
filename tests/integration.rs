//! Offline integration test for the ergonomic API: Locator, expect assertions,
//! page assertions and typed events. Uses `set_content`, so no network.
//!
//! The test needs the Playwright driver and a Chromium binary. If the driver
//! can't be located (e.g. in an environment without it installed), the test
//! skips rather than failing.

use std::time::Duration;

use playwright::{expect, expect_page, Playwright};

const PAGE: &str = r#"<!doctype html><title>Integration</title>
<h1 id="heading">Hello Playwright</h1>
<ul><li class="item">a</li><li class="item">b</li><li class="item">c</li></ul>
<input id="name" type="text" data-role="field" />
<button id="hidden" style="display:none">nope</button>
<button id="go">Click Me</button>
<span data-testid="status">ready</span>
<iframe srcdoc="<p id='inner'>inside frame</p>"></iframe>"#;

#[tokio::test]
async fn ergonomic_end_to_end() {
    let pw = match Playwright::initialize().await {
        Ok(pw) => pw,
        Err(e) => {
            eprintln!("skipping: Playwright driver unavailable: {e}");
            return;
        }
    };

    let browser = pw
        .chromium()
        .await
        .unwrap()
        .launch(Default::default())
        .await
        .unwrap();
    let context = browser.new_context(Default::default()).await.unwrap();
    let mut console = context.on_console().await;
    let page = context.new_page().await.unwrap();
    page.set_content(PAGE).await.unwrap();

    // --- Locator basics ---
    let heading = page.locator("#heading").await.unwrap();
    assert_eq!(heading.count().await.unwrap(), 1);
    assert_eq!(heading.inner_text().await.unwrap(), "Hello Playwright");

    let items = page.locator("li.item").await.unwrap();
    assert_eq!(items.count().await.unwrap(), 3);
    assert_eq!(items.nth(1).text_content().await.unwrap().as_deref(), Some("b"));
    assert_eq!(items.all_text_contents().await.unwrap(), vec!["a", "b", "c"]);
    assert_eq!(items.all().await.unwrap().len(), 3);

    let field = page.locator("#name").await.unwrap();
    field.fill("Ferris").await.unwrap();
    assert_eq!(field.input_value().await.unwrap(), "Ferris");
    assert_eq!(
        field.get_attribute("data-role").await.unwrap().as_deref(),
        Some("field")
    );

    let bbox = heading.bounding_box().await.unwrap();
    assert!(bbox.map(|b| b.width > 0.0).unwrap_or(false), "heading should have a box");

    // --- Locator assertions ---
    expect(&heading).to_be_visible().await.unwrap();
    expect(&heading).to_have_text("Hello Playwright").await.unwrap();
    expect(&heading).to_contain_text("Playwright").await.unwrap();
    expect(&items).to_have_count(3).await.unwrap();
    expect(&field).to_have_value("Ferris").await.unwrap();
    expect(&field).to_have_attribute("data-role", "field").await.unwrap();
    expect(&page.locator("#hidden").await.unwrap()).to_be_hidden().await.unwrap();
    expect(&heading).not().to_be_hidden().await.unwrap();

    // A failing assertion must report an error (short timeout to stay fast).
    let failed = expect(&heading)
        .timeout(Duration::from_millis(300))
        .to_have_text("Wrong")
        .await;
    assert!(failed.is_err(), "mismatched text should fail");

    // --- Page assertions ---
    expect_page(&page).to_have_title("Integration").await.unwrap();

    // --- Typed events ---
    page.evaluate("() => console.log('integration log')").await.unwrap();
    let msg = tokio::time::timeout(Duration::from_secs(5), console.recv()).await;
    match msg {
        Ok(Some(m)) => {
            assert_eq!(m.r#type, "log");
            assert_eq!(m.text, "integration log");
        }
        other => panic!("expected a console message, got {other:?}"),
    }

    // --- get_by_* selectors ---
    assert_eq!(
        page.get_by_test_id("status").await.unwrap().inner_text().await.unwrap(),
        "ready"
    );
    assert_eq!(
        page.get_by_role("button", Some("Click Me")).await.unwrap().count().await.unwrap(),
        1
    );
    assert!(page.get_by_text("Hello Playwright").await.unwrap().count().await.unwrap() >= 1);

    // --- frame_locator ---
    let inner = page
        .frame_locator("iframe")
        .await
        .unwrap()
        .locator("#inner")
        .inner_text()
        .await
        .unwrap();
    assert_eq!(inner, "inside frame");

    // --- Tracing (start, do work, stop to a zip) ---
    let tracing = context.tracing().await.unwrap();
    tracing.start(Some("itest"), true, true).await.unwrap();
    page.evaluate("() => 1 + 1").await.unwrap();
    let trace_path = std::env::temp_dir().join("pw-rs-itest-trace.zip");
    let trace_str = trace_path.to_string_lossy().to_string();
    tracing.stop(Some(&trace_str)).await.unwrap();
    assert!(trace_path.exists(), "trace zip should be written");
    let _ = std::fs::remove_file(&trace_path);

    // --- Clock (fake time) ---
    let clock_ctx = browser.new_context(Default::default()).await.unwrap();
    let clock = clock_ctx.clock();
    clock.install().await.unwrap();
    clock.set_fixed_time(2_000.0).await.unwrap();
    let clock_page = clock_ctx.new_page().await.unwrap();
    clock_page.set_content("<p>clock</p>").await.unwrap();
    let now = clock_page.evaluate("() => Date.now()").await.unwrap();
    assert_eq!(now, serde_json::json!(2000), "set_fixed_time should freeze Date.now()");
    clock_ctx.close(Default::default()).await.unwrap();

    // --- Keyboard / Mouse ---
    let field2 = page.locator("#name").await.unwrap();
    field2.clear().await.unwrap();
    field2.click().await.unwrap();
    page.keyboard().type_text("typed!").await.unwrap();
    expect(&field2).to_have_value("typed!").await.unwrap();
    // Mouse smoke: should not error.
    page.mouse().move_to(5.0, 5.0).await.unwrap();

    // --- Network routing (offline: fulfilled locally) ---
    let rpage = context.new_page().await.unwrap();
    rpage
        .route("**/*", |route| async move {
            let _ = route
                .fulfill_text(200, "text/html", "<title>Routed</title><h1 id=\"r\">routed page</h1>")
                .await;
        })
        .await
        .unwrap();
    let resp = rpage.goto("https://routed.test/").await.unwrap();
    if let Some(resp) = resp {
        assert_eq!(resp.status().await.unwrap(), 200);
        assert!(resp
            .headers()
            .await
            .unwrap()
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case("content-type") && v.contains("text/html")));
    }
    expect_page(&rpage).to_have_title("Routed").await.unwrap();
    assert_eq!(
        rpage.locator("#r").await.unwrap().inner_text().await.unwrap(),
        "routed page"
    );
    // unroute should not error (removing one pattern, then all).
    rpage.unroute(Some("**/*")).await.unwrap();
    rpage.unroute(None).await.unwrap();

    browser.close(Default::default()).await.unwrap();
    pw.stop().await;
}
