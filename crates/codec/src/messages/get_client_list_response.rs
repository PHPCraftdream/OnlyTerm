use mux::client::ClientInfo;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct GetClientListResponse {
    pub clients: Vec<ClientInfo>,
}
