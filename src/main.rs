mod app;
mod block;
mod chain;
mod p2p;

use tracing::Level;
use tracing_subscriber::FmtSubscriber;

use crate::app::App;

// TODO: see the article for details on adding a retry mechanism for failed blocks

fn init_logging() -> anyhow::Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_logging()?;

    let app = App::default();
    app.run().await
}
