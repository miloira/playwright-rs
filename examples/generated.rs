//! Drives the fully generated protocol channel layer directly (no ergonomic
//! wrappers), proving the generated `protocol` module works end to end. Offline:
//! uses `setContent` instead of a network navigation.

use playwright::protocol::{
    Browser, BrowserContext, BrowserType, BrowserTypeLaunchParams, Frame, FrameSetContentParams,
    Page,
};
use playwright::Playwright;

#[tokio::main]
async fn main() -> playwright::Result<()> {
    let pw = Playwright::initialize().await?;
    let conn = pw.connection();

    // Root Playwright channel -> its initializer carries the engine guids.
    let root = pw.channel();
    let init = root.initializer().await?;

    let chromium = BrowserType::from_channel(conn.clone(), init.chromium.guid);
    println!("engine: {}", chromium.initializer().await?.name);

    let launched = chromium
        .launch(BrowserTypeLaunchParams {
            mixin: playwright::protocol::LaunchOptions {
                headless: Some(true),
                ..Default::default()
            },
            ..Default::default()
        })
        .await?;
    let browser = Browser::from_channel(conn.clone(), launched.browser.guid);
    println!("browser version: {}", browser.initializer().await?.version);

    let context = BrowserContext::from_channel(
        conn.clone(),
        browser.newContext(Default::default()).await?.context.guid,
    );
    let page = Page::from_channel(conn.clone(), context.newPage().await?.page.guid);

    // Page-level DOM methods live on the main frame.
    let main_frame = Frame::from_channel(conn.clone(), page.initializer().await?.mainFrame.guid);
    main_frame
        .setContent(FrameSetContentParams {
            html: "<title>Generated</title><h1>via generated channels</h1>".to_string(),
            waitUntil: None,
        })
        .await?;

    println!("title: {}", main_frame.title().await?.value);

    browser.close(Default::default()).await?;
    pw.stop().await;
    println!("GENERATED LAYER OK");
    Ok(())
}
