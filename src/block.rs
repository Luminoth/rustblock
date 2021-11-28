use chrono::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::task;
use tracing::{debug, info, warn};

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
async fn calculate_hash(
    id: u64,
    timestamp: i64,
    previous_block_hash: impl Into<String>,
    data: impl Into<String>,
    nonce: u64,
) -> anyhow::Result<Vec<u8>> {
    let previous_block_hash = previous_block_hash.into();
    let data = data.into();

    let res = task::spawn_blocking(move || {
        let data = serde_json::json!({
            "id": id,
            "previous_hash": previous_block_hash,
            "data": data,
            "timestamp": timestamp,
            "nonce": nonce
        });

        let mut hasher = Sha256::new();
        hasher.update(data.to_string().as_bytes());

        hasher.finalize().as_slice().to_owned()
    })
    .await?;

    Ok(res)
}

/// Mines a new block, returning the nonce and hash that were mined
async fn mine_block(
    id: u64,
    timestamp: i64,
    previous_block_hash: impl Into<String>,
    data: impl Into<String>,
    difficulty_prefix: impl AsRef<str>,
) -> anyhow::Result<(u64, String)> {
    let previous_block_hash = previous_block_hash.into();
    let data = data.into();

    info!("mining block...");
    let mut nonce = 0;

    // TODO: add timing to this

    loop {
        // log every 100,000th nonce
        if nonce % 100_000 == 0 {
            debug!("nonce: {}", nonce);
        }

        // calculate the hash with the nonce and see if it meets our required difficulty
        let hash = calculate_hash(id, timestamp, &previous_block_hash, &data, nonce).await?;

        let binary_hash = hash_to_binary_representation(&hash);
        if binary_hash.starts_with(difficulty_prefix.as_ref()) {
            info!(
                "mined! nonce: {}, hash: {}, binary hash: {}",
                nonce,
                hex::encode(&hash),
                binary_hash
            );
            return Ok((nonce, hex::encode(hash)));
        }

        nonce += 1;
    }
}

/// Holds data for a block in the blockchain
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Block {
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
    pub fn genesis() -> Self {
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
    pub async fn new(
        id: u64,
        previous_block_hash: impl Into<String>,
        data: impl Into<String>,
        difficulty_prefix: impl AsRef<str>,
    ) -> anyhow::Result<Self> {
        let previous_block_hash = previous_block_hash.into();
        let data = data.into();

        let now = Utc::now().timestamp();
        let (nonce, hash) =
            mine_block(id, now, &previous_block_hash, &data, difficulty_prefix).await?;

        Ok(Self {
            id,
            hash,
            timestamp: now,
            previous_block_hash,
            data,
            nonce,
        })
    }

    /// Returns the block id
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Returns the block's hash
    pub fn hash(&self) -> &String {
        &self.hash
    }

    /// Checks if this block is valid given a previous block
    pub async fn is_valid(
        &self,
        previous_block: &Block,
        difficulty_prefix: impl AsRef<str>,
    ) -> anyhow::Result<bool> {
        // validate the block id
        if self.id != previous_block.id + 1 {
            warn!(
                "block with id: {} is not the next block after the latest: {}",
                self.id, previous_block.id
            );
            return Ok(false);
        }

        // validate the previous block hash
        if self.previous_block_hash != previous_block.hash {
            warn!("block with id: {} has wrong previous hash", self.id);
            return Ok(false);
        }

        // ensure the block hash meets our difficulty criteria
        // (decoding should be faster than calculating the hash)
        let decoded = hex::decode(&self.hash)?;
        let bin = hash_to_binary_representation(&decoded);
        if !bin.starts_with(difficulty_prefix.as_ref()) {
            warn!("block with id: {} has invalid difficulty", self.id);
            return Ok(false);
        }

        // validate the block hash
        let hash = calculate_hash(
            self.id,
            self.timestamp,
            &self.previous_block_hash,
            &self.data,
            self.nonce,
        )
        .await?;
        if hex::encode(hash) != self.hash {
            warn!("block with id: {} has invalid hash", self.id);
            return Ok(false);
        }

        Ok(true)
    }
}
