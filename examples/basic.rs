//! Launches Chromium, navigates to a page, reads its title/content, runs a
//! small script, and saves a screenshot. Requires the Chromium binary:
//!
//! ```text
//! node driver/node_modules/playwright-core/cli.js install chromium
//! ```

use playwright::Playwright;

#[tokio::main]
async fn main() -> playwright::Result<()> {
    let pw = Playwright::initialize().await?;
    let chromium = pw.chromium().await?;
    let browser = chromium.launch(Default::default()).await?;
    println!("browser version: {}", browser.initializer().await?.version);

    let page = browser.new_page().await?;

    if let Some(resp) = page.goto("https://example.com").await? {
        println!("GET {} -> {}", resp.url().await?, resp.status().await?);
    }

    println!("title: {}", page.title().await?);
    println!("url: {}", page.url().await?);
    println!("h1 text: {:?}", page.text_content("h1").await?);

    let ua = page.evaluate("() => navigator.userAgent").await?;
    println!("user agent: {ua}");

    let shot = page.screenshot_png().await?;
    std::fs::write("example.png", &shot)?;
    println!("wrote example.png ({} bytes)", shot.len());

    browser.close(Default::default()).await?;
    pw.stop().await;
    Ok(())
}
