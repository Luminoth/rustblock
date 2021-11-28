use anyhow::bail;
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::block::Block;

/// Checks if the blockchain is valid
async fn is_chain_valid(chain: impl AsRef<[Block]>) -> anyhow::Result<bool> {
    for i in 1..chain.as_ref().len() {
        let first = chain.as_ref().get(i - 1).unwrap();
        let second = chain.as_ref().get(i).unwrap();
        if !second.is_valid(first).await? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Choose the longest valid chain from the given set
// TODO: see the article for details on going deeper here
async fn choose_chain(local: Vec<Block>, remote: Vec<Block>) -> anyhow::Result<Vec<Block>> {
    let is_local_valid = is_chain_valid(&local).await?;
    let is_remote_valid = is_chain_valid(&remote).await?;

    Ok(if is_local_valid && is_remote_valid {
        if local.len() >= remote.len() {
            local
        } else {
            remote
        }
    } else if is_remote_valid && !is_local_valid {
        remote
    } else if !is_remote_valid && is_local_valid {
        local
    } else {
        bail!("local and remote chains are both invalid");
    })
}

/// Holds the blockchain
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Chain {
    /// The blockchain
    blocks: Vec<Block>,
}

impl Chain {
    /// Create a new, empty blockchain
    pub fn new() -> Self {
        Self { blocks: Vec::new() }
    }

    /// Returns a reference to the chain's blocks
    pub fn blocks(&self) -> &Vec<Block> {
        &self.blocks
    }

    /// Create the genesis (initial) block in the chain
    pub fn genesis(&mut self) {
        let genesis_block = Block::genesis();
        self.blocks.push(genesis_block);
    }

    /// Returns the last block in the chain
    pub fn last(&self) -> Option<&Block> {
        self.blocks.last()
    }

    /// Attempt to add a block to the chain
    pub async fn try_add_block(&mut self, block: Block) -> anyhow::Result<bool> {
        // only add the new block if it's valid against the latest block
        let latest_block = self.blocks.last().unwrap();
        if block.is_valid(latest_block).await? {
            self.blocks.push(block);
            Ok(true)
        } else {
            // TODO: error handling
            error!("could not add block - invalid");
            Ok(false)
        }
    }

    /// Sets this chain's blocks to the remote chain if it is longer
    pub async fn choose_chain(&mut self, remote: Vec<Block>) -> anyhow::Result<()> {
        self.blocks = choose_chain(self.blocks.clone(), remote).await?;

        Ok(())
    }
}
