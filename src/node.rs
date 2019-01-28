use num_bigint::BigInt;

use tokio::io;
use tokio::net::{TcpStream, TcpListener};
use tokio::prelude::*;

use futures::{Future, Stream};

use std::net::SocketAddr;
use std::io::BufReader;

use std::{thread, time, str};

use super::finger::FingerTable;
use super::finger;
use super::network_util;
use super::protocols::*;
use super::storage::Storage;
use super::util::*;
use super::chord;

/// Simple representation of an external node in the network
#[derive(Clone, Serialize, Deserialize, Debug)]
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
    internal_name: String,
    id: BigInt,
    ip_addr: SocketAddr,
    // finger_table: FingerTable, TODO we will care about this later
    // next_finger: usize,
    successor: OtherNode,
    predecessor: Option<OtherNode>,
    joined: bool,
    // storage: Storage,
}

/// `Node` implementation
impl Node {
    /// Creates new Node
    /// TODO fix comments
    /// if `predecessor` is None, the node will initialize a new chord ring
    /// if `predecessor` is Some(), the node will join an existing network and `predecessor` as its own predecessor
    ///
    /// * `ip_addr`     - Ip address and port of the node
    /// * `predecessor` - (Optional) Ip address and port of a known member of an existing network
    pub fn new(internal_name: String, node_ip_addr: SocketAddr, entry_node_addr: SocketAddr) -> Node {
        //let next_finger = 0; // Always start at first entry of finger_table
        //let finger_table = FingerTable::new(successor.clone(), &id);
        //let storage = Storage::new();
        Node {
            internal_name: internal_name,
            id: create_node_id(node_ip_addr),
            ip_addr: node_ip_addr,
            successor: OtherNode { id: create_node_id(entry_node_addr), ip_addr: entry_node_addr },
            predecessor: None,
            joined: false,
        }
    }

    pub fn new_first(internal_name: String, node_ip_addr: SocketAddr) -> Node {
        let id = create_node_id(node_ip_addr);
        Node {
            internal_name: internal_name,
            id: id.clone(),
            ip_addr: node_ip_addr.clone(),
            successor: OtherNode { id: id.clone(), ip_addr: node_ip_addr.clone() },
            predecessor: Some(OtherNode { id: id, ip_addr: node_ip_addr }),
            joined: true,
        }
    }

    pub fn join(&mut self) {
        info!("Starting joining process");
        let req = Request::FindSuccessor { id: self.id.clone() };
        self.send_message_to_socket(self.successor.ip_addr, req);
    }

    pub fn start_stabilisation(&mut self) {
        let mut node_clone = self.clone();
        let builder = thread::Builder::new().name(format!("{}-Stabilize", node_clone.internal_name).to_string());
        let handler = builder
            .spawn(move || {
                info!("Starting stabilisation");
                loop {
                    let req = Request::GetPredecessor;
                    node_clone.send_message_to_socket(node_clone.successor.ip_addr, req);
                    thread::sleep(chord::NODE_STABILIZE_INTERVAL);
                }
            })
            .unwrap();
    }

    /// Converts internal representation of node to the simpler representation OtherNode
    fn to_other_node(&self) -> OtherNode {
        OtherNode {
            id: self.id.clone(),
            ip_addr: self.ip_addr,
        }
    }

    fn process_incoming_request(&mut self, request: Request) -> Response {
        match request {
            Request::FindSuccessor { id } => {
                self.handle_find_successor_request(id)
            }
            Request::GetPredecessor => {
                self.handle_get_predecessor_request()
            }
            Request::Notify { node } => {
                self.handle_notify_request(node)
            }
        }
    }

    fn handle_find_successor_request(&self, id: BigInt) -> Response {
        if is_in_half_range(&id, &self.id, self.successor.get_id()) {
            Response::FoundSuccessor { successor: self.successor.clone() }
        } else {
            Response::AskFurther { next_node: self.successor.clone() }
        }
    }

    fn handle_get_predecessor_request(&self) -> Response {
        Response::GetPredecessorResponse {
            predecessor: self.predecessor.clone()
        }
    }

    fn handle_notify_request(&mut self, node: OtherNode) -> Response {
        match &self.predecessor {
            None => {
                info!("[Node #{}] Predecessor is now: {}",self.id , node.id);
                self.predecessor = Some(node)
            }
            Some(pre) => {
                println!("[{:p} - {}] Current pre id: {}, possible new pre id: {}", self, self.id, pre.id, node.id);
                if pre.id != node.id && is_in_range(node.get_id(), pre.get_id(), &self.id) {
                    info!("[Node #{}] Predecessor is now: {}",self.id , node.id);
                    self.predecessor = Some(node);
                    println!("Predecessor: {}", self.predecessor.clone().unwrap().id);
                }
            }
        }
        //TODO check if maybe a failure notification is necessary
        Response::NotifyResponse
    }

    fn process_incoming_response(&mut self, response: Response) {
        match response {
            Response::FoundSuccessor { successor } => {
                self.handle_found_successor_response(successor)
            }
            Response::AskFurther { next_node } => {
                self.handle_ask_further_response(next_node)
            }
            Response::GetPredecessorResponse { predecessor } => {
                self.handle_get_predecessor_response(predecessor)
            }
            Response::NotifyResponse => {
                self.handle_notify_response()
            }
        }
    }

    fn handle_found_successor_response(&mut self, successor: OtherNode) {
        info!("Found my new successor: node #{}", successor.id.clone());
        self.successor = successor;
        if !self.joined {
            self.start_stabilisation();
            self.joined = true;
        }
    }

    fn handle_ask_further_response(&mut self, next_node: OtherNode) {
        info!("Did not get successor yet, asking node #{} now...", next_node.id);
        let req = Request::FindSuccessor { id: self.id.clone() };
        self.send_message_to_socket(next_node.ip_addr, req);
    }

    fn handle_get_predecessor_response(&mut self, predecessor: Option<OtherNode>) {
        if let Some(predecessor) = predecessor {
            if is_in_range(predecessor.get_id(), &self.id, self.successor.get_id()) {
                info!("Successor was node #{}, but got node #{} as predecessor of successor, so it is  successor now...", self.successor.id.clone(), predecessor.id.clone());
                self.successor = predecessor;
            }
        }
        let req = Request::Notify { node: self.to_other_node() };
        self.send_message_to_socket(self.successor.ip_addr, req);
    }

    fn handle_notify_response(&self) {}
    

    // HINT: this can be tested by connecting via bash terminal (preinstalled on Mac/Linux) by executing:
    // nc 127.0.0.1 34254
    // afterwards every message will be echoed in the console by handle_request
    pub fn start_listening_on_socket(&mut self) -> Result<(), Box<std::error::Error>> {
        let mut node = self.clone();
        let listener = TcpListener::bind(&self.ip_addr).unwrap();

        //TODO figure out if extensive cloning is working
        info!("[Node #{}] Starting to listen on socket: {}", self.id, self.ip_addr);

        let server = listener.incoming().for_each(move |socket| {
            info!("[Node #{}] accepted socket; addr={:?}", node.id.clone(), socket.peer_addr()?);

            let buf = vec![];
            let buf_reader = BufReader::new(socket);
            let mut node_clone = node.clone();
            let connection = io::read_until(buf_reader, b'\n', buf)
                .and_then(move |(socket, buf)| {
                    let stream = socket.into_inner();

                    let msg_string = str::from_utf8(&buf).unwrap();

                    let message: RequestMessage = serde_json::from_str(msg_string).unwrap();
                    let request: Request = message.request;
                    info!("[Node #{}] Got request from Node #{}: {:?}", node_clone.id.clone(), message.sender.id, request.clone());
                    let response: Response = node_clone.process_incoming_request(request);
                    info!("[Node #{}] Sending response: {:?}", node_clone.id.clone(), response.clone());
                    let response_message = ResponseMessage { sender: node_clone.to_other_node(), response };
                    let response_string = format!("{}\n", serde_json::to_string(&response_message).unwrap());
                    io::write_all(stream, response_string)
                })
                .then(|_| Ok(())); // Just discard the socket and buffer

            // Spawn a new task that processes the socket:
            tokio::spawn(connection);

            Ok(())
        }).map_err(|e| println!("failed to accept socket; error = {:?}", e));
        tokio::run(server);
        Ok(())
    }

    pub fn send_message_to_socket(&mut self, addr: SocketAddr, request: Request) {
        let node = self.clone();
        let builder = thread::Builder::new().name(self.internal_name.clone().to_string());
        builder
            .spawn(move || {
                let mut node = node.clone();
                let request_message = RequestMessage { sender: node.to_other_node(), request };
                let request_message_string: String = format!("{}\n", serde_json::to_string(&request_message).unwrap());
                let client = TcpStream::connect(&addr).and_then(move |stream| {
                    io::write_all(stream, request_message_string).and_then(move |(stream, msg)| {
                        let sock = BufReader::new(stream);
                        io::read_until(sock, b'\n', vec![]).and_then(move |(stream, buf)| {
                            let response_string = str::from_utf8(&buf).unwrap();
                            let response_message: ResponseMessage = serde_json::from_str(response_string).unwrap();
                            let response: Response = response_message.response;
                            info!("[Node #{}] Got response from Node #{}: {:?}", node.id.clone(), response_message.sender.id, response.clone());
                            node.process_incoming_response(response);
                            Ok(())
                        })
                    })
                })
                    .map_err(|err| {
                        println!("connection error = {:?}", err);
                    });
                tokio::run(client);
                //Ok(())
            });
    }
}
