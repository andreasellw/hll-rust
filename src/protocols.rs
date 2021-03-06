use num_bigint::BigInt;

use super::node::OtherNode;
use super::storage::DHTEntry;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Message {
    Kill,
    Ping {
        sender: OtherNode
    },
    RequestMessage {
        sender: OtherNode,
        request: Request,
    },
    ResponseMessage {
        sender: OtherNode,
        response: Response,
    },
}


#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Request {
    FindSuccessor {
        id: BigInt
    },
    GetPredecessor,
    Notify {
        node: OtherNode,
    },
    FindSuccessorFinger {
        index: usize,
        finger_id: BigInt,
    },
    GetSuccessorList,
    DHTStoreKey {
        data: (BigInt, DHTEntry)
    },
    DHTFindKey {
        key_id: BigInt
    },
    DHTDeleteKey {
        key_id: BigInt
    },
    DHTTakeOverKeys {
        data: Vec<(BigInt, DHTEntry)>
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Response {
    FoundSuccessor {
        successor: OtherNode
    },
    AskFurther {
        next_node: OtherNode
    },
    GetPredecessorResponse {
        predecessor: Option<OtherNode>
    },
    NotifyResponse,
    FoundSuccessorFinger {
        index: usize,
        finger_id: BigInt,
        successor: OtherNode,
    },
    AskFurtherFinger {
        index: usize,
        finger_id: BigInt,
        next_node: OtherNode,
    },
    GetSuccessorListResponse {
        successor_list: Vec<OtherNode>
    },
    DHTStoredKey{
        key: String,
    },
    DHTFoundKey {
        data: (BigInt, Option<DHTEntry>)
    },
    DHTDeletedKey {
        key_existed: bool
    },
    DHTAskFurtherStore {
        next_node: OtherNode,
        data: (BigInt, DHTEntry),
    },
    DHTAskFurtherFind {
        next_node: OtherNode,
        key_id: BigInt,
    },
    DHTAskFurtherDelete {
        next_node: OtherNode,
        key_id: BigInt,
    },
}

