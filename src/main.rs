use chrono::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Chain {
    blocks: Vec<Block>,
}

impl Chain {
    fn new() -> Self {
        Self { blocks: Vec::new() }
    }

    fn genesis(&mut self) {
        let genesis_block = Block::genesis();
        self.blocks.push(genesis_block);
    }

    fn try_add_block(&mut self, block: Block) {
        let latest_block = self.blocks.last().unwrap();
        if self.is_block_valid(&block, latest_block) {
            self.blocks.push(block);
        } else {
            error!("could not add block - invalid");
        }
    }

    fn is_block_valid(&self, block: &Block, previous_block: &Block) -> bool {
        false
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Block {
    id: u64,
    hash: String,
    previous_block_hash: String,
    timestamp: i64,
    data: String,
    nonce: u64,
}

fn init_logging() -> anyhow::Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;

    Ok(())
}

impl Block {
    fn genesis() -> Self {
        Self {
            id: 0,
            timestamp: Utc::now().timestamp(),
            previous_block_hash: String::from("genesis"),
            data: String::from("genesis!"),
            nonce: 2836,
            hash: "0000f816a87f806bb0073dcf026a64fb40c946b5abee2573702828694d5b4c43".to_string(),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_logging()?;

    info!("Hello, world!");

    Ok(())
}
