mod block;
mod chain;

use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use crate::block::Block;
use crate::chain::Chain;

// TODO: see the article for details on adding a retry mechanism for failed blocks

// TODO: this is overly simplified
// see the article for more details on how to go deeper here
// ("00" is relatively easy and quick to mine)
const DIFFICULTY_PREFIX: &str = "00";

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

    info!("starting ...");

    let mut chain = Chain::new();
    chain.genesis();

    loop {
        let latest_block = chain.last().unwrap();

        let block = Block::new(
            latest_block.id() + 1,
            latest_block.hash(),
            "test",
            &DIFFICULTY_PREFIX,
        )
        .await?;

        chain.try_add_block(block, &DIFFICULTY_PREFIX).await?;
    }
}
