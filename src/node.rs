use std::net::SocketAddr;

use super::finger::FingerTable;
use super::network::Network;
use super::protocols::*;
use super::storage::Storage;
use super::util::*;
use num_bigint::BigInt;

#[derive(Clone)]
pub struct OtherNode {
    id: BigInt,
    ip_addr: SocketAddr,
}

impl OtherNode {
    pub fn new(id: BigInt, ip: SocketAddr) -> OtherNode {
        return OtherNode { id, ip_addr: ip };
    }

    pub fn print(&self, desc: &str) {
        info!("{}: id: {}, ip_addr: {}", desc, self.id, self.ip_addr);
    }

    pub fn get_id(&self) -> &BigInt {
        &self.id
    }

    pub fn get_ip_addr(&self) -> &SocketAddr {
        &self.ip_addr
    }
}

pub struct Node {
    id: BigInt,
    ip_addr: SocketAddr,
    network: Network,
    //TODO check if better possibilities available
    predecessor: Option<OtherNode>,
    successor: OtherNode,      //TODO can be found out by finger table
    finger_table: FingerTable, //TODO do we need finger_entries (e.g. 32 or 8)
    storage: Storage,
    next_finger: usize,
}

impl Node {
    //Constructor for initialisation of new Chord Ring, call new_existing_network if joining existing network
    pub fn new(ip_addr: SocketAddr) -> Node {
        let id = create_node_id(ip_addr);
        let successor = OtherNode::new(id.clone(), ip_addr);
        let finger_table = FingerTable::new();
        let storage = Storage::new();
        /*  TODO fix when new is implemented
            TODO In addition to that we need to check how network cann call methods on node, particularly: process_received_msg
        */
        let network = Network::new(ip_addr);
        let next_finger = 0;

        info!("Node: id: {}, ip_addr: {}", id, ip_addr);
        successor.print("Successor");

        return Node {
            id,
            ip_addr: ip_addr,
            predecessor: None,
            successor,
            finger_table,
            storage,
            network,
            next_finger,
        };
    }

    //TODO check if needs to be pulic method, assumption: No ;)
    pub fn to_other_node(&self) -> OtherNode {
        return OtherNode {
            id: self.id.clone(),
            ip_addr: self.ip_addr.clone(),
        };
    }

    pub fn closet_finger_preceding(&self, find_id: BigInt) -> OtherNode {
        /*
         * n.closest_preceding_node(id)
         *   for i = m downto 1
         *     if (finger[i]∈(n,id))
         *       return finger[i];
         *   return n;
         */
        for x in self.finger_table.length()..0 {
            let finger_entry = self.finger_table.get(x);
            match finger_entry {
                Some(finger_entry) => {
                    if is_in_range(finger_entry.node.get_id(), &self.id, &find_id) {
                        return finger_entry.node.clone();
                    }
                }
                _ => {}
            }
        }

        if is_in_range(&self.successor.id, &self.id, &find_id) {
            return self.successor.clone();
        } else {
            return self.to_other_node();
        }
    }

    pub fn send_msg(&self, _from: OtherNode, _to: Option<OtherNode>, _message: Message) {
        let from = _from;

        let to = match _to {
            Some(to) => to,
            None => from.clone(),
        };

        let mut message = _message;
        if message.get_id().is_none() {
            message.set_id(Some(self.id.clone()))
        }

        //TODO build JSON Object, and send it as message

        self.network.send(from, to, message);
    }

    //TODO find better name
    pub fn start_network(self) {
        self.network.start_listening_on_socket();
    }

    pub fn process_received_msg(&mut self, _from: OtherNode, _message: Message) {
        let from = _from;
        let mut message = _message;

        match message.get_message_type() {
            // Node notifies successor about predecessor
            NOTIFY_PREDECESSOR =>
            /*
             *  predecessor is nil or n'∈(predecessor, n)
             */
            {
                info!("0-NOTIFY_PREDECESSOR");
                message.print();
                let pre_to_send = match self.predecessor.clone() {
                    Some(predecessor) => {
                        if is_in_range(&from.id, &predecessor.id, &self.id) {
                            from.print("New predecessor ist now");
                            self.predecessor = Some(from.clone());
                            from.clone()
                        } else {
                            predecessor
                        }
                    }
                    None => {
                        from.print("New predecessor ist now");
                        self.predecessor = Some(from.clone());
                        from.clone()
                    }
                };
                self.send_msg(pre_to_send, Some(from), message);
            }

            // Stabilize
            NOTIFY_SUCCESSOR =>
            /*
             *  n.stabilize()
             *    x = successor.predecessor;
             *    if (x∈(n, successor))
             *      successor = x;
             *    successor.notify(n);
             */
            {
                info!("1-NOTIFY_SUCCESSOR");
                message.print();

                if is_in_range(&from.id, &self.id, &self.successor.id) {
                    self.successor = from;
                    self.successor.print("New succesor is now");
                }
            }
            NOTIFY_JOIN => {
                info!("2-NOTIFY_JOIN");
                message.print();
                from.print("Node joined");
            }
            FIND_SUCCESSOR => {
                info!("3-FIND_SUCCESSOR");
                message.print();
                message.get_id().map(|id| {
                    if is_in_half_range(&id, &self.id, &self.successor.id) {
                        self.successor.print("FIND_SUCCESSOR");
                        message.set_message_type(FOUND_SUCCESSOR);
                        self.send_msg(self.successor.clone(), Some(from), message);
                    } else {
                        let node_0 = self.closet_finger_preceding(id);
                        self.successor.print("FIND_SUCCESSOR = closet_finger_preceding");
                        message.set_message_type(FOUND_SUCCESSOR);
                        self.send_msg(node_0, Some(from), message);
                    }
                });
            }
            FOUND_SUCCESSOR => {
                info!("4-FOUND_SUCCESSOR");
                message.print();

                match (message.get_next_finger(), message.get_id()) {
                    (Some(next_finger), Some(id)) => {
                        self.finger_table.put(next_finger, id, from);
                        info!("FingerTable fixed");
                    }
                    _ => {
                        self.successor = from;
                        self.successor.print("New successor is now");
                    }
                }
            }
            MESSAGE => {
                info!("5-MESSAGE");
                self.send_msg(self.successor.clone(), Some(from), message);
            }
            _ => {
                warn!("Unknown chord message!");
                message.print();
            }
        }
    }
}
