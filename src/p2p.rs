use std::collections::HashSet;

use libp2p::floodsub::*;
use libp2p::mdns::*;
use libp2p::swarm::*;
use libp2p::*;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
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
pub struct Behavior {
    /// P2P protocol instance
    floodsub: Floodsub,

    /// mDNS for node discovery
    mdns: Mdns,

    /// Init event sender
    #[behaviour(ignore)]
    init_sender: mpsc::UnboundedSender<()>,

    /// Chain response sender
    #[behaviour(ignore)]
    response_sender: mpsc::UnboundedSender<ChainResponse>,

    /// Our blockchain
    #[behaviour(ignore)]
    chain: Chain,
}

impl Behavior {
    /// Creates a new behavior
    pub async fn new(
        chain: Chain,
        init_sender: mpsc::UnboundedSender<()>,
        response_sender: mpsc::UnboundedSender<ChainResponse>,
    ) -> anyhow::Result<Self> {
        let mut behaviour = Self {
            floodsub: Floodsub::new(*PEER_ID),
            mdns: Mdns::new(Default::default()).await?,
            init_sender,
            response_sender,
            chain,
        };
        behaviour.floodsub.subscribe(CHAIN_TOPIC.clone());
        behaviour.floodsub.subscribe(BLOCK_TOPIC.clone());

        Ok(behaviour)
    }
}

/// Handle Mdns events
impl NetworkBehaviourEventProcess<MdnsEvent> for Behavior {
    fn inject_event(&mut self, event: MdnsEvent) {
        match event {
            // add new nodes
            MdnsEvent::Discovered(discovered_list) => {
                for (peer, _addr) in discovered_list {
                    self.floodsub.add_node_to_partial_view(peer);
                }
            }
            // remove expired nodes
            MdnsEvent::Expired(expired_list) => {
                for (peer, _addr) in expired_list {
                    if !self.mdns.has_node(&peer) {
                        self.floodsub.remove_node_from_partial_view(&peer);
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

                    self.chain.blocks = self
                        .chain
                        .choose_chain(self.chain.blocks.clone(), resp.blocks);
                }
            } else if let Ok(resp) = serde_json::from_slice::<LocalChainRequest>(&msg.data) {
                info!("sending local chain to {}", msg.source.to_string());
                let peer_id = resp.from_peer_id;
                if PEER_ID.to_string() == peer_id {
                    if let Err(e) = self.response_sender.send(ChainResponse {
                        blocks: self.chain.blocks.clone(),
                        receiver: msg.source.to_string(),
                    }) {
                        error!("error sending response via channel, {}", e);
                    }
                }
            } else if let Ok(block) = serde_json::from_slice::<Block>(&msg.data) {
                info!("received new block from {}", msg.source.to_string());
                self.chain.try_add_block(block);
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

pub fn handle_print_chain(swarm: &Swarm<Behavior>) {
    info!("Local Blockchain:");

    let pretty_json = serde_json::to_string_pretty(&swarm.behaviour().chain.blocks).unwrap();
    info!("{}", pretty_json);
}

pub fn handle_create_block(cmd: impl AsRef<str>, swarm: &mut Swarm<Behavior>) {
    if let Some(data) = cmd.strip_prefix("create b") {
        let behaviour = swarm.behaviour_mut();
        let latest_block = behaviour.chain.blocks.last().unwrap();
        let block = Block::new(
            latest_block.id + 1,
            latest_block.hash.clone(),
            data.to_owned(),
        );
        let json = serde_json::to_string(&block).expect("can jsonify request");
        behaviour.chain.blocks.push(block);
        info!("broadcasting new block");
        behaviour
            .floodsub
            .publish(BLOCK_TOPIC.clone(), json.as_bytes());
    }
}
