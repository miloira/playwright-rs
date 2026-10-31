//! Exercises the client-side abstractions: Locator, expect assertions, and
//! typed context events. Offline (uses set_content).

use std::time::Duration;

use playwright::{expect, Playwright};

#[tokio::main]
async fn main() -> playwright::Result<()> {
    let pw = Playwright::initialize().await?;
    let browser = pw.chromium().await?.launch(Default::default()).await?;
    let context = browser.new_context(Default::default()).await?;

    // Subscribe before creating the page so no console message is missed.
    let mut console = context.on_console().await;

    let page = context.new_page().await?;
    page.set_content(
        r#"<!doctype html><title>Loc</title>
        <h1 id="heading">Hello Playwright</h1>
        <ul><li class="item">a</li><li class="item">b</li><li class="item">c</li></ul>
        <input id="name" type="text" />
        <button id="missing" style="display:none">hidden</button>"#,
    )
    .await?;

    // --- Locator ---
    let heading = page.locator("#heading").await?;
    println!("heading count: {}", heading.count().await?);
    println!("heading text: {}", heading.inner_text().await?);
    assert_eq!(heading.count().await?, 1);

    let items = page.locator("li.item").await?;
    println!("item count: {}", items.count().await?);
    assert_eq!(items.count().await?, 3);
    println!("second item: {:?}", items.nth(1).text_content().await?);
    assert_eq!(items.nth(1).text_content().await?.as_deref(), Some("b"));

    let name = page.locator("#name").await?;
    name.fill("Ferris").await?;
    assert_eq!(name.input_value().await?, "Ferris");
    println!("input value: {}", name.input_value().await?);

    // --- expect assertions ---
    expect(&heading).to_be_visible().await?;
    expect(&heading).to_have_text("Hello Playwright").await?;
    expect(&heading).to_contain_text("Playwright").await?;
    expect(&items).to_have_count(3).await?;
    expect(&name).to_have_value("Ferris").await?;
    expect(&page.locator("#missing").await?).to_be_hidden().await?;
    expect(&heading).not().to_be_hidden().await?;
    println!("all assertions passed");

    // An assertion that should fail, to prove failures are reported (short
    // timeout so we don't wait the full default).
    match expect(&heading)
        .timeout(std::time::Duration::from_millis(500))
        .to_have_text("Wrong Text")
        .await
    {
        Err(e) => println!("expected failure reported: {e}"),
        Ok(()) => panic!("assertion should have failed"),
    }

    // --- typed events ---
    page.evaluate("() => console.log('hi from the page')").await?;
    match tokio::time::timeout(Duration::from_secs(5), console.recv()).await {
        Ok(Some(msg)) => println!("console [{}]: {}", msg.r#type, msg.text),
        _ => println!("no console message received"),
    }

    browser.close(Default::default()).await?;
    pw.stop().await;
    println!("LOCATOR / EXPECT / EVENTS OK");
    Ok(())
}
