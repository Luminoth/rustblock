use chrono::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{error, info, warn, Level};
use tracing_subscriber::FmtSubscriber;

// TODO: see the article for details on adding a retry mechanism for failed blocks

// TODO: this is overly simplified
// see the article for more details on how to go deeper here
// ("00" is relatively easy and quick to mine)
const DIFFICULTY_PREFIX: &str = "00";

/// Convert a hash to its binary string representation
// TODO: this is an inefficient way to check the difficulty prefix of a hash
// see the article for more details on how to go deeper here
fn hash_to_binary_representation(hash: impl AsRef<[u8]>) -> String {
    hash.as_ref()
        .iter()
        .map(|c| format!("{:b}", c))
        .collect::<Vec<String>>()
        .join("")
}

/// Calculates the hash of a block's JSON representation
fn calculate_hash(
    id: u64,
    timestamp: i64,
    previous_hash: impl AsRef<str>,
    data: impl AsRef<str>,
    nonce: u64,
) -> Vec<u8> {
    let data = serde_json::json!({
        "id": id,
        "previous_hash": previous_hash.as_ref(),
        "data": data.as_ref(),
        "timestamp": timestamp,
        "nonce": nonce
    });

    let mut hasher = Sha256::new();
    hasher.update(data.to_string().as_bytes());

    hasher.finalize().as_slice().to_owned()
}

/// Mines a new block, returning the nonce and hash that were mined
fn mine_block(
    id: u64,
    timestamp: i64,
    previous_block_hash: impl AsRef<str>,
    data: impl AsRef<str>,
) -> (u64, String) {
    info!("mining block...");
    let mut nonce = 0;

    loop {
        // log every 100,000th nonce
        if nonce % 100_000 == 0 {
            info!("nonce: {}", nonce);
        }

        // calculate the hash with the nonce and see if it meets our required difficulty
        let hash = calculate_hash(id, timestamp, &previous_block_hash, &data, nonce);
        let binary_hash = hash_to_binary_representation(&hash);
        if binary_hash.starts_with(DIFFICULTY_PREFIX) {
            info!(
                "mined! nonce: {}, hash: {}, binary hash: {}",
                nonce,
                hex::encode(&hash),
                binary_hash
            );
            return (nonce, hex::encode(hash));
        }

        nonce += 1;
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Chain {
    /// The blockchain
    blocks: Vec<Block>,
}

impl Chain {
    /// Checks if the given block is valid
    fn is_block_valid(block: &Block, previous_block: &Block) -> bool {
        // validate the block id
        if block.id != previous_block.id + 1 {
            warn!(
                "block with id: {} is not the next block after the latest: {}",
                block.id, previous_block.id
            );
            return false;
        }

        // validate the previous block hash
        if block.previous_block_hash != previous_block.hash {
            warn!("block with id: {} has wrong previous hash", block.id);
            return false;
        }

        // ensure the block hash meets our difficulty criteria
        // (decoding should be faster than calculating the hash)
        let decoded = match hex::decode(&block.hash) {
            Ok(decoded) => decoded,
            Err(e) => {
                error!("cannot decode from hex: {}", e);
                return false;
            }
        };
        let bin = hash_to_binary_representation(&decoded);
        if !bin.starts_with(DIFFICULTY_PREFIX) {
            warn!("block with id: {} has invalid difficulty", block.id);
            return false;
        }

        // validate the block hash
        let hash = calculate_hash(
            block.id,
            block.timestamp,
            &block.previous_block_hash,
            &block.data,
            block.nonce,
        );
        if hex::encode(hash) != block.hash {
            warn!("block with id: {} has invalid hash", block.id);
            return false;
        }

        true
    }

    /// Checks if the blockchain is valid
    #[allow(dead_code)]
    fn is_chain_valid(chain: impl AsRef<[Block]>) -> bool {
        // TODO: there's probably an iterator method for this right?
        for i in 1..chain.as_ref().len() {
            let first = chain.as_ref().get(i - 1).unwrap();
            let second = chain.as_ref().get(i).unwrap();
            if !Self::is_block_valid(second, first) {
                return false;
            }
        }
        true
    }

    /// Choose the longest valid chain from the given set
    // TODO: see the article for details on going deeper here
    #[allow(dead_code)]
    fn choose_chain(local: Vec<Block>, remote: Vec<Block>) -> Vec<Block> {
        let is_local_valid = Self::is_chain_valid(&local);
        let is_remote_valid = Self::is_chain_valid(&remote);

        if is_local_valid && is_remote_valid {
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
            panic!("local and remote chains are both invalid");
        }
    }

    /// Create a new blockchain
    fn new() -> Self {
        Self { blocks: Vec::new() }
    }

    /// Create the genesis (initial) block in the chain
    fn genesis(&mut self) {
        let genesis_block = Block::genesis();
        self.blocks.push(genesis_block);
    }

    /// Attempt to add a block to the chain
    fn try_add_block(&mut self, block: Block) {
        // only add the new block if it's valid against the latest block
        let latest_block = self.blocks.last().unwrap();
        if Self::is_block_valid(&block, latest_block) {
            self.blocks.push(block);
        } else {
            // TODO: error handling
            error!("could not add block - invalid");
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Block {
    /// The block id (index)
    id: u64,

    /// The block's sha256 hash
    hash: String,

    /// The sha256 hash of the previous block
    previous_block_hash: String,

    /// The block's creation timestamp
    timestamp: i64,

    /// The block's data
    data: String,

    /// The block's nonce
    nonce: u64,
}

impl Block {
    /// Creates a new genesis (initial) block
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

    /// Mine's a new block
    pub fn new(id: u64, previous_block_hash: impl Into<String>, data: impl Into<String>) -> Self {
        let previous_block_hash = previous_block_hash.into();
        let data = data.into();

        let now = Utc::now();
        let (nonce, hash) = mine_block(id, now.timestamp(), &previous_block_hash, &data);
        Self {
            id,
            hash,
            timestamp: now.timestamp(),
            previous_block_hash,
            data,
            nonce,
        }
    }
}

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

    let mut chain = Chain::new();
    chain.genesis();

    loop {
        let latest_block = chain.blocks.last().unwrap();

        let block = Block::new(latest_block.id + 1, &latest_block.hash, "test");
        chain.try_add_block(block);
    }
}
