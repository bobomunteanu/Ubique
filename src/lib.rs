#![allow(unused)]

use async_std::io;
use futures::executor::block_on;
use futures::prelude::*;
use libp2p::ping::{Behaviour};
use std::error::Error;
use std::task::Poll;
use futures::{prelude::*, select};
use libp2p::kad::record::store::MemoryStore;
use libp2p::kad::{
    record::Key, AddProviderOk, Kademlia, KademliaEvent, PeerRecord, PutRecordOk, QueryResult,
    Quorum, Record,
};
use libp2p::{
    identity,
    mdns::{Mdns, MdnsConfig, MdnsEvent},
    swarm::SwarmEvent,
    NetworkBehaviour, PeerId, Swarm,
};
use libp2p::{development_transport, Multiaddr};

macro_rules! aw {
    ($e:expr) => {
        tokio_test::block_on($e)
    };
}

pub fn gen_key() -> identity::Keypair {
    let local_key = identity::Keypair::generate_ed25519();
    local_key
}

pub fn gen_peer_id(local_key: identity::Keypair) -> PeerId {
    let local_peer_id = PeerId::from(local_key.clone().public());
    local_peer_id
}

pub fn recieve_ping() -> Result<(), Box<dyn Error>> {
    //Generate a keypair for the device
    let local_key = gen_key();

    //Generates a peer ID
    let local_peer_id = PeerId::from(local_key.public());
    println!("Local peer id: {:?}", local_peer_id);

    //Creates a transport using the local key
    let transport = block_on(libp2p::development_transport(local_key))?;

    //Configures the behaviour and creates the Swarm
    let mut swarm = Swarm::new(transport, Behaviour::default(), local_peer_id);

    //Starts the Swarm
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;


    let mut listening = false;
    block_on(future::poll_fn(move |cx| loop {
        match swarm.poll_next_unpin(cx) {
            Poll::Ready(Some(event)) => println!("{:?}", event),
            Poll::Ready(None) => return Poll::Ready(()),
            Poll::Pending => {
                if !listening {
                    for addr in Swarm::listeners(&swarm) {
                        println!("Listening on {}", addr);
                        listening = true;
                    }
                }
                return Poll::Pending;
            }
        }
    }));

    Ok(())
}

pub fn send_ping(addr:String) -> Result<(), Box<dyn Error>> {
    //Generate a keypair for the device
    let local_key = gen_key();

    //Generates a peer ID
    let local_peer_id =  PeerId::from(local_key.public());
    println!("Local peer id: {:?}", local_peer_id);

    //Creates a transport using the local key
    let transport = block_on(libp2p::development_transport(local_key))?;

    //Configures the behaviour and creates the Swarm
    let mut swarm = Swarm::new(transport, Behaviour::default(), local_peer_id);

    //Starts the swarm
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    //Creates an address object from the parameter string and dials it using the created swarm ('tunes' it to listen on an ip & port)
    let remote: Multiaddr = addr.parse()?;
    swarm.dial(remote)?;
    println!("Dialed {}", addr);


    let mut listening = false;
    block_on(future::poll_fn(move |cx| loop {
        match swarm.poll_next_unpin(cx) {
            Poll::Ready(Some(event)) => println!("{:?}", event),
            Poll::Ready(None) => return Poll::Ready(()),
            Poll::Pending => {
                if !listening {
                    for addr in Swarm::listeners(&swarm) {
                        println!("Listening on {}", addr);
                        listening = true;
                    }
                }
                return Poll::Pending;
            }
        }
    }));

    Ok(())
}

pub async fn keyvalue_store() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    let local_key = gen_key();
    let local_peer_id = gen_peer_id(local_key.clone());

    let transport = development_transport(local_key).await?;

    // Create a custom network behaviour with Kademlia and MDNS.

    #[derive(NetworkBehaviour)]
    #[behaviour(out_event = "BehaviourEvent")]
    struct Behaviour {
        kademlia: Kademlia<MemoryStore>,
        mdns: Mdns,
    }

    #[allow(clippy::large_enum_variant)]
    enum BehaviourEvent {
        Kademlia(KademliaEvent),
        Mdns(MdnsEvent),
    }

    impl From<KademliaEvent> for BehaviourEvent {
        fn from(event: KademliaEvent) -> Self {
            BehaviourEvent::Kademlia(event)
        }
    }

    impl From<MdnsEvent> for BehaviourEvent {
        fn from(event: MdnsEvent) -> Self {
            BehaviourEvent::Mdns(event)
        }
    }

    // Create a swarm to manage peers and events.
    let mut swarm = {
        // Create a Kademlia behaviour.
        let store = MemoryStore::new(local_peer_id);
        let kademlia = Kademlia::new(local_peer_id, store);
        let mdns = Mdns::new(MdnsConfig::default())?;
        let behaviour = Behaviour { kademlia, mdns };
        Swarm::new(transport, behaviour, local_peer_id)
    };

    // Read full lines from stdin
    let mut stdin = io::BufReader::new(io::stdin()).lines().fuse();

    // Listen on all interfaces and whatever port the OS assigns.
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    loop {
        select! {
            line = stdin.select_next_some() => handle_input_line(&mut swarm.behaviour_mut().kademlia, line.expect("Stdin not to close")),
            event = swarm.select_next_some() => match event {
                SwarmEvent::NewListenAddr { address, .. } => {
                    println!("Listening in {:?}", address);
                },
                SwarmEvent::Behaviour(BehaviourEvent::Mdns(MdnsEvent::Discovered(list))) => {
                        for (peer_id, multiaddr) in list {
                            swarm.behaviour_mut().kademlia.add_address(&peer_id, multiaddr);
                        }
                }
                SwarmEvent::Behaviour(BehaviourEvent::Kademlia(KademliaEvent::OutboundQueryCompleted { result, ..})) => {
                    match result {
                        QueryResult::GetProviders(Ok(ok)) => {
                            for peer in ok.providers {
                                println!(
                                        "Peer {:?} provides key {:?}",
                                peer,
                                std::str::from_utf8(ok.key.as_ref()).unwrap()
                                );
                            }
                        }
                    QueryResult::GetProviders(Err(err)) => {
                            eprintln!("Failed to get providers: {:?}", err);
                        }
                    QueryResult::GetRecord(Ok(ok)) => {
                            for PeerRecord {
                                record: Record { key, value, .. },
                            ..
                            } in ok.records
                    {
                        println!(
                                "Got record {:?} {:?}",
                        std::str::from_utf8(key.as_ref()).unwrap(),
                        std::str::from_utf8(&value).unwrap(),
                        );
                    }
                        }
                    QueryResult::GetRecord(Err(err)) => {
                            eprintln!("Failed to get record: {:?}", err);
                        }
                    QueryResult::PutRecord(Ok(PutRecordOk { key })) => {
                            println!(
                                    "Successfully put record {:?}",
                            std::str::from_utf8(key.as_ref()).unwrap()
                            );
                        }
                    QueryResult::PutRecord(Err(err)) => {
                            eprintln!("Failed to put record: {:?}", err);
                        }
                    QueryResult::StartProviding(Ok(AddProviderOk { key })) => {
                            println!(
                                    "Successfully put provider record {:?}",
                            std::str::from_utf8(key.as_ref()).unwrap()
                            );
                        }
                    QueryResult::StartProviding(Err(err)) => {
                            eprintln!("Failed to put provider record: {:?}", err);
                        }
                    _ => {}
                    }
                }
        _ => {}
            }
        }
    }

    Ok(())
}

fn handle_input_line(kademlia: &mut Kademlia<MemoryStore>, line: String) {
    let mut args = line.split(' ');

    match args.next() {
        Some("GET") => {
            let key = {
                match args.next() {
                    Some(key) => Key::new(&key),
                    None => {
                        eprintln!("Expected key");
                        return;
                    }
                }
            };
            kademlia.get_record(key, Quorum::One);
        }
        Some("GET_PROVIDERS") => {
            let key = {
                match args.next() {
                    Some(key) => Key::new(&key),
                    None => {
                        eprintln!("Expected key");
                        return;
                    }
                }
            };
            kademlia.get_providers(key);
        }
        Some("PUT") => {
            let key = {
                match args.next() {
                    Some(key) => Key::new(&key),
                    None => {
                        eprintln!("Expected key");
                        return;
                    }
                }
            };
            let value = {
                match args.next() {
                    Some(value) => value.as_bytes().to_vec(),
                    None => {
                        eprintln!("Expected value");
                        return;
                    }
                }
            };
            let record = Record {
                key,
                value,
                publisher: None,
                expires: None,
            };
            kademlia
            .put_record(record, Quorum::One)
            .expect("Failed to store record locally.");
        }
        Some("PUT_PROVIDER") => {
            let key = {
                match args.next() {
                    Some(key) => Key::new(&key),
                    None => {
                        eprintln!("Expected key");
                        return;
                    }
                }
            };

            kademlia
            .start_providing(key)
            .expect("Failed to start providing key");
        }
        _ => {
            eprintln!("expected GET, GET_PROVIDERS, PUT or PUT_PROVIDER");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() -> Result<(), Box<dyn Error>> {
        aw!(keyvalue_store());
        Ok(())
    }
}
