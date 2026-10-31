//! Offline verification that needs no network: sets page HTML directly, then
//! exercises title, DOM reads, form fill, evaluate and screenshot.

use playwright::Playwright;

#[tokio::main]
async fn main() -> playwright::Result<()> {
    let pw = Playwright::initialize().await?;
    let chromium = pw.chromium().await?;
    let browser = chromium.launch(Default::default()).await?;
    println!("browser version: {}", browser.initializer().await?.version);

    let page = browser.new_page().await?;

    page.set_content(
        r#"<!doctype html>
        <html><head><title>Offline Test</title></head>
        <body>
            <h1 id="heading">Hello Playwright</h1>
            <a id="link" href="https://rust-lang.org">a link</a>
            <input id="name" type="text" />
            <button id="btn" onclick="document.getElementById('heading').textContent='clicked'">go</button>
        </body></html>"#,
    )
    .await?;

    assert_eq!(page.title().await?, "Offline Test");
    println!("title: {}", page.title().await?);
    println!("h1 inner_text: {}", page.inner_text("#heading").await?);
    println!("h1 text_content: {:?}", page.text_content("#heading").await?);
    println!("link href: {:?}", page.get_attribute("#link", "href").await?);

    page.fill("#name", "Ferris").await?;
    let typed = page.evaluate("() => document.getElementById('name').value").await?;
    println!("input value after fill: {typed}");
    assert_eq!(typed, serde_json::json!("Ferris"));

    page.click("#btn").await?;
    page.wait_for_selector("#heading").await?;
    let after = page.inner_text("#heading").await?;
    println!("h1 after click: {after}");
    assert_eq!(after, "clicked");

    let math = page.evaluate_with("(x) => x * 2", serde_json::json!(21)).await?;
    println!("21 * 2 = {math}");
    assert_eq!(math, serde_json::json!(42));

    let shot = page.screenshot_png().await?;
    std::fs::write("offline.png", &shot)?;
    println!("screenshot bytes: {}", shot.len());
    assert!(shot.len() > 100, "screenshot looks empty");

    browser.close(Default::default()).await?;
    pw.stop().await;
    println!("ALL OFFLINE CHECKS PASSED");
    Ok(())
}
