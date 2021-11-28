use std::collections::HashSet;
use std::sync::Arc;

use libp2p::{floodsub::*, identity, mdns::*, swarm::*, NetworkBehaviour, PeerId};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, RwLock};
use tracing::{error, info};

use crate::block::Block;
use crate::chain::Chain;

// TODO: floodsub is not the most efficient protocol
// see the article for details on going deeper here

// libp2p peer identity
pub static KEYS: Lazy<identity::Keypair> = Lazy::new(identity::Keypair::generate_ed25519);
pub static PEER_ID: Lazy<PeerId> = Lazy::new(|| PeerId::from(KEYS.public()));

// libp2p topics
pub static CHAIN_TOPIC: Lazy<Topic> = Lazy::new(|| Topic::new("chains"));
pub static BLOCK_TOPIC: Lazy<Topic> = Lazy::new(|| Topic::new("blocks"));

/// Chain response
#[derive(Debug, Serialize, Deserialize)]
pub struct ChainResponse {
    pub blocks: Vec<Block>,
    pub receiver: String,
}

/// Request a chain from a specific peer
#[derive(Debug, Serialize, Deserialize)]
pub struct LocalChainRequest {
    pub from_peer_id: String,
}

/// Event types
pub enum EventType {
    LocalChainResponse(ChainResponse),
    Input(String),
    Init,
}

#[derive(NetworkBehaviour)]
#[behaviour(event_process = true)]
pub struct Behavior {
    /// P2P protocol instance
    protocol: Floodsub,

    /// mDNS for node discovery
    mdns: Mdns,

    /// Chain response sender
    #[behaviour(ignore)]
    response_sender: mpsc::UnboundedSender<ChainResponse>,

    /// Our blockchain
    #[behaviour(ignore)]
    chain: Arc<RwLock<Chain>>,
}

impl Behavior {
    /// Creates a new behavior
    pub async fn new(
        chain: Chain,
        response_sender: mpsc::UnboundedSender<ChainResponse>,
    ) -> anyhow::Result<Self> {
        let mut behaviour = Self {
            protocol: Floodsub::new(*PEER_ID),
            mdns: Mdns::new(Default::default()).await?,
            response_sender,
            chain: Arc::new(RwLock::new(chain)),
        };

        behaviour.protocol.subscribe(CHAIN_TOPIC.clone());
        behaviour.protocol.subscribe(BLOCK_TOPIC.clone());

        Ok(behaviour)
    }

    pub fn chain(&self) -> Arc<RwLock<Chain>> {
        self.chain.clone()
    }

    pub fn protocol_mut(&mut self) -> &mut Floodsub {
        &mut self.protocol
    }
}

/// Handle Mdns events
impl NetworkBehaviourEventProcess<MdnsEvent> for Behavior {
    fn inject_event(&mut self, event: MdnsEvent) {
        match event {
            // add new nodes
            MdnsEvent::Discovered(discovered_list) => {
                for (peer, _addr) in discovered_list {
                    self.protocol.add_node_to_partial_view(peer);
                }
            }
            // remove expired nodes
            MdnsEvent::Expired(expired_list) => {
                for (peer, _addr) in expired_list {
                    if !self.mdns.has_node(&peer) {
                        self.protocol.remove_node_from_partial_view(&peer);
                    }
                }
            }
        }
    }
}

/// Hadle Floodsub events
impl NetworkBehaviourEventProcess<FloodsubEvent> for Behavior {
    fn inject_event(&mut self, event: FloodsubEvent) {
        if let FloodsubEvent::Message(msg) = event {
            if let Ok(resp) = serde_json::from_slice::<ChainResponse>(&msg.data) {
                if resp.receiver == PEER_ID.to_string() {
                    info!("Response from {}:", msg.source);
                    resp.blocks.iter().for_each(|r| info!("{:?}", r));

                    let chain = self.chain.clone();
                    tokio::spawn(async move {
                        chain.write().await.choose_chain(resp.blocks).await.unwrap();
                    });
                }
            } else if let Ok(resp) = serde_json::from_slice::<LocalChainRequest>(&msg.data) {
                info!("sending local chain to {}", msg.source.to_string());
                let peer_id = resp.from_peer_id;
                if PEER_ID.to_string() == peer_id {
                    let response_sender = self.response_sender.clone();
                    let chain = self.chain.clone();
                    tokio::spawn(async move {
                        if let Err(e) = response_sender.send(ChainResponse {
                            blocks: chain.read().await.blocks().clone(),
                            receiver: msg.source.to_string(),
                        }) {
                            error!("error sending response via channel, {}", e);
                        }
                    });
                }
            } else if let Ok(block) = serde_json::from_slice::<Block>(&msg.data) {
                info!("received new block from {}", msg.source.to_string());

                let chain = self.chain.clone();
                tokio::spawn(async move {
                    chain.write().await.try_add_block(block).await.unwrap();
                });
            }
        }
    }
}

pub fn get_list_peers(swarm: &Swarm<Behavior>) -> Vec<String> {
    info!("Discovered Peers:");

    let nodes = swarm.behaviour().mdns.discovered_nodes();
    let mut unique_peers = HashSet::new();
    for peer in nodes {
        unique_peers.insert(peer);
    }
    unique_peers.iter().map(|p| p.to_string()).collect()
}

pub fn handle_print_peers(swarm: &Swarm<Behavior>) {
    let peers = get_list_peers(swarm);
    peers.iter().for_each(|p| info!("{}", p));
}

pub async fn handle_print_chain(swarm: &Swarm<Behavior>) {
    info!("Local Blockchain:");

    let pretty_json =
        serde_json::to_string_pretty(&swarm.behaviour().chain.read().await.blocks()).unwrap();
    info!("{}", pretty_json);
}

pub async fn handle_create_block(
    cmd: impl AsRef<str>,
    swarm: &mut Swarm<Behavior>,
) -> anyhow::Result<()> {
    if let Some(data) = cmd.as_ref().strip_prefix("create b") {
        let behaviour = swarm.behaviour_mut();

        let block = {
            let chain = behaviour.chain.read().await;
            let latest_block = chain.last().unwrap();
            Block::new(
                latest_block.id() + 1,
                latest_block.hash().clone(),
                data.to_owned(),
            )
            .await?
        };

        let json = serde_json::to_string(&block).unwrap();

        if behaviour.chain.write().await.try_add_block(block).await? {
            info!("broadcasting new block");
            behaviour
                .protocol
                .publish(BLOCK_TOPIC.clone(), json.as_bytes());
        }
    }

    Ok(())
}
