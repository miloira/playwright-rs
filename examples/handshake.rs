//! Verifies the driver launch and protocol handshake without needing browser
//! binaries: it initializes Playwright and prints each engine's name.

use playwright::Playwright;

#[tokio::main]
async fn main() -> playwright::Result<()> {
    let pw = Playwright::initialize().await?;

    for engine in ["chromium", "firefox", "webkit"] {
        let bt = match engine {
            "chromium" => pw.chromium().await?,
            "firefox" => pw.firefox().await?,
            _ => pw.webkit().await?,
        };
        println!("{engine}: name={:?}", bt.initializer().await?.name);
    }

    pw.stop().await;
    println!("handshake OK");
    Ok(())
}
