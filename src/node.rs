use num_bigint::BigInt;
use std::io::{BufRead, BufReader};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::{thread, time};

use super::finger::FingerTable;
use super::finger;
use super::network_util;
use super::protocols::*;
use super::storage::Storage;
use super::util::*;
use super::chord;

/// Simple representation of an external node in the network
#[derive(Clone, Serialize, Deserialize)]
pub struct OtherNode {
    id: BigInt,
    ip_addr: SocketAddr,
}

impl OtherNode {
    pub fn new(id: BigInt, ip: SocketAddr) -> OtherNode {
        OtherNode { id, ip_addr: ip }
    }

    pub fn get_id(&self) -> &BigInt {
        &self.id
    }

    pub fn get_ip_addr(&self) -> &SocketAddr {
        &self.ip_addr
    }

    pub fn print(&self, desc: &str) {
        info!("{}: id: {}, ip_addr: {}", desc, self.id, self.ip_addr);
    }
}

/// Complete representation of internal node
///
/// * `id`             - Identifier of node: Currently SHA1 hashed IP address
/// * `ip_addr`        - Ip address and port of the node
/// * `finger_table`   - Finger table of the node, which stores up to n other nodes
/// * `next_finger`    - Used to point on the entry of the finger table, we are currently processing
/// * `successor`      - Successor of the node //TODO can be found out by finger table, //TODO do we need var finger_entries (e.g. 32 or 8) -> Not really, finger_entries depend on bit_size of hashing!!
/// * `predecessor`    - [Optional] Predecessor of the node
/// * `storage`        - DHT storage inside the node
#[derive(Clone)]
pub struct Node {
    id: BigInt,
    ip_addr: SocketAddr,
    finger_table: FingerTable,
    next_finger: usize,
    successor: OtherNode,
    predecessor: Option<OtherNode>,
    storage: Storage,
}

/// `Node` implementation
impl Node {
    /// Creates new Node
    /// if `predecessor` is None, the node will initialize a new chord ring
    /// if `predecessor` is Some(), the node will join an existing network and `predecessor` as its own predecessor
    ///
    /// * `ip_addr`     - Ip address and port of the node
    /// * `predecessor` - (Optional) Ip address and port of a known member of an existing network
    // TODO implement predecessor: Option<SocketAddr>
    pub fn new(ip_addr: String, port: i32, initial_successor: Option<SocketAddr>) -> Node {
        let ip_addr = format!("{}:{}", ip_addr, port)
            .parse::<SocketAddr>()
            .unwrap();
        let id = create_node_id(ip_addr);
        // Always start at first entry of finger_table
        let next_finger = 0;
        let successor = if let Some(successor) = initial_successor {
            OtherNode::new(create_node_id(successor), successor)
        } else {
            OtherNode::new(id.clone(), ip_addr)
        };
        let finger_table = FingerTable::new(successor.clone());

        let storage = Storage::new();
        debug!("New node {:?}", id);
        Node {
            id,
            ip_addr,
            finger_table,
            next_finger,
            successor,
            predecessor: None,
            storage,
        }
    }

    /// Converts internal representation of node to the simpler representation OtherNode
    fn to_other_node(&self) -> OtherNode {
        OtherNode {
            id: self.id.clone(),
            ip_addr: self.ip_addr,
        }
    }

    /// Gets closet preceding finger
    pub fn closet_finger_preceding(&self, find_id: &BigInt) -> OtherNode {
        // n.closest_preceding_node(id)
        //   for i = m downto 1
        //     if (finger[i]∈(n,id))
        //       return finger[i];
        //   return n;
        for x in self.finger_table.length()..0 {
            let finger_entry = self.finger_table.get(x);
            if let Some(finger_entry) = finger_entry {
                if is_in_range(finger_entry.node.get_id(), &self.id, &find_id) {
                    return finger_entry.node.clone();
                }
            }
        }

        if is_in_range(&self.successor.id, &self.id, &find_id) {
            self.successor.clone()
        } else {
            self.to_other_node()
        }
    }

    /// Entry point after creation of node
    /// Loops periodically to update fingertable
    /// Calls fix_fingers
    /// Notifies successor that I am his predecessor by sending NOTIFY_PREDECESSOR
    pub fn start_update_fingers(&mut self) {
        loop {
            debug!("start_update_fingers()");
            self.fix_fingers();
            let message = Message::new(NOTIFY_PREDECESSOR, None, None);
            self.send_msg(self.successor.clone(), None, message);

            let millis2000 = time::Duration::from_millis(2000);
            let now = time::Instant::now();
            thread::sleep(millis2000);
            assert!(now.elapsed() >= millis2000);
        }
    }

    /// Periodically find successor for all entries of our fingertable
    /// Sending self a message which subsequently sends messages to others
    /// by dispatching FIND_SUCCESSOR message to other nodes
    fn fix_fingers(&mut self) {
        let fix_finger_id: BigInt;
        let mut next = self.next_finger;
        //next >= self.finger_table.length()
        if next >= chord::FINGERTABLE_SIZE {
            next = 0;
        }
        fix_finger_id = finger::get_finger_id(&self.id, next);
        self.next_finger = next + 1;
        // n.fix_fingers()
        let message = Message::new(FIND_SUCCESSOR, Some(next), Some(fix_finger_id));
        self.send_msg(self.to_other_node(), None, message);
    }

    /// Notifies other peer about joining the network
    /// TODO set remote to our successor
    pub fn join(&mut self, remote: OtherNode) -> bool {
        let message = Message::new(NOTIFY_JOIN, None, None);
        self.predecessor = None;
        remote.print("Try to join");
        self.send_msg(remote, None, message);
        true
    }

    pub fn send_msg(&self, mut label: OtherNode, to: Option<OtherNode>, mut msg: Message) {
        // If no recipient is provided,
        // the message is returned to the intial sender
        // and labelled by `self` as `from`
        let new_to = match to {
            Some(to) => to,
            None => {
                let new_to = label.clone();
                label = self.to_other_node();
                new_to
            }
        };

        // If the message id is undefined, it is set to `self``s ID
        if msg.get_id().is_none() {
            msg.set_id(Some(self.id.clone()))
        }

        let packet = Packet::new(label, msg);
        let json_string = serde_json::to_string(&packet).unwrap();
        // Send packet to recipient
        network_util::send_string_to_socket(*new_to.get_ip_addr(), json_string);
    }

    fn handle_request(&mut self, stream: TcpStream, client_addr: SocketAddr) {
        let mut reader = BufReader::new(stream);

        loop {
            let mut buffer = String::new();
            match reader.read_line(&mut buffer) {
                Ok(len) => {
                    // break when line is finished
                    if len == 0 {
                        break;
                    } else {
                        info!("New message from {}: {}", client_addr.to_string(), buffer);
                        let parsed_packet: Packet = serde_json::from_str(&buffer).unwrap();
                        let from = parsed_packet.get_from();
                        let message = parsed_packet.get_message();
                        self.process_incoming_msg(from.clone(), message.clone());
                    }
                }
                Err(e) => error!("Error reading message from {}: {}", client_addr, e),
            }
        }
    }

    // HINT: this can be tested by connecting via bash terminal (preinstalled on Mac/Linux) by executing:
    // nc 127.0.0.1 34254
    // afterwards every message will be echoed in the console by handle_request
    pub fn start_listening_on_socket(&mut self) {
        let listener = TcpListener::bind(self.ip_addr).unwrap();
        info!("Started listening on {}", self.ip_addr.to_string());
        loop {
            match listener.accept() {
                Ok((stream, addr)) => {
                    info!("Connection by {}", addr.to_string());

                    self.handle_request(stream, addr);
                }
                Err(e) => error!("Connection failed: {:?}", e),
            };
        }
    }

    //pub fn network(&self) -> &Network{
    //    &self.network.unwrap()
    //}

    pub fn process_incoming_msg(&mut self, from: OtherNode, msg: Message) {
        match msg.get_message_type() {
            NOTIFY_PREDECESSOR => self.update_predecessor(from, msg),
            NOTIFY_SUCCESSOR => self.update_successor(from, msg),
            NOTIFY_JOIN => self.notify_join(from, msg),
            FIND_SUCCESSOR => self.find_successor(from, msg),
            FOUND_SUCCESSOR => self.found_successor(from, msg),
            MESSAGE => self.message(from, msg),
            _ => {
                warn!("Unknown chord message!");
                msg.print();
            }
        }
    }

    /// A node `from` claims, that it is self's _new_ predecessor
    /// ```rust
    /// n.notify(n')
    ///   if ( predecessor is nil or n' ∈ (predecessor, n) )
    ///     predecessor = n';
    /// ```
    fn update_predecessor(&mut self, from: OtherNode, mut msg: Message) {
        info!("MSG_TYPE_NOTIFY_PREDECESSOR = 0");

        // Copy current self.predecessor value
        let current_predecessor = self.predecessor.clone();
        // Reassign self.predecessor
        let new_predecessor = match current_predecessor {
            // If `self.current_predecessor` is not empty verify
            // if `from` is in range
            // else keep `current_predecessor`
            Some(self_predecessor) => {
                if is_in_range(&from.id, &self_predecessor.id, &self.id) {
                    from.print("Predecessor reassigned to");
                    self.predecessor = Some(from.clone());
                    from.clone()
                } else {
                    info!("Predecessor remains the same.");
                    self_predecessor
                }
            }
            // If `self.predecessor` is nil, assign `from` as new predecessor
            None => {
                from.print("Predecessor assigned to");
                self.predecessor = Some(from.clone());
                from.clone()
            }
        };
        msg.set_message_type(NOTIFY_SUCCESSOR);
        // TODO WHAT THE FUCK why is this msg labelled by the new_predecessor?
        self.send_msg(new_predecessor, Some(from), msg);
        self.finger_table.print()
    }

    /// ```rust
    /// n.stabilize()
    ///   x = successor.predecessor;
    ///   if( x ∈ (n, successor) )
    ///     successor = x;
    ///   successor.notify(n);
    /// ```
    fn update_successor(&mut self, from: OtherNode, _msg: Message) {
        info!("MSG_TYPE_NOTIFY_SUCCESSOR = 1");

        // TODO maybe delete successor field in node struct
        // TODO and instead use first finger entry in fingertable
        // TODO but we have to verify if we don't produce any unforseen changes in the implementation
        if is_in_range(&from.id, &self.id, &self.successor.id) {
            self.successor = from;
            self.successor.print("Successor reassigned");
        }
    }

    fn notify_join(&mut self, from: OtherNode, _msg: Message) {
        info!("MSG_TYPE_NOTIFY_JOIN = 2");
        from.print("Node joined");
    }

    /// ```rust
    /// n.find_successor(id)
    ///  if ( id ∈ (n, successor] )
    ///    return successor;
    ///  else
    ///    return successor.find_successor(id);
    /// ```
    fn find_successor(&mut self, from: OtherNode, mut msg: Message) {
        info!("MSG_TYPE_FIND_SUCCESSOR = 3");

        if let Some(msg_id) = msg.get_id() {
            if is_in_half_range(&msg_id, &self.id, &self.successor.id) {
                self.successor.print("FIND_SUCCESSOR");
                msg.set_message_type(FOUND_SUCCESSOR);
                self.send_msg(self.successor.clone(), Some(from), msg);
            } else {
                // Fix fingertable and forward the query
                let node_0 = self.closet_finger_preceding(&msg_id);
                node_0.print("FIND_SUCCESSOR = closet_finger_preceding");
                msg.set_message_type(FOUND_SUCCESSOR);
                self.send_msg(node_0, Some(from), msg);
            }
        };
    }

    /// ```rust
    /// n.fix_fingers()
    ///   for i = 1 to m
    ///     finger[i].Knoten = find_successor(finger[i].Start);
    /// ```
    fn found_successor(&mut self, from: OtherNode, msg: Message) {
        info!("MSG_TYPE_FOUND_SUCCESSOR = 4");

        match (msg.get_next_finger(), msg.get_id()) {
            (Some(index), Some(id)) => {
                // indexOutOfBounds concern? -> not possible, because of implementation of put()
                self.finger_table.put(index, id, from);
                info!("FingerTable fixed.");
                self.finger_table.print();
            }
            _ => {
                self.successor = from;
                self.successor.print("New successor is now");
            }
        }
    }

    fn message(&mut self, from: OtherNode, msg: Message) {
        info!("MSG_TYPE_MESSAGE = 5");

        self.send_msg(self.successor.clone(), Some(from), msg);
    }
}
