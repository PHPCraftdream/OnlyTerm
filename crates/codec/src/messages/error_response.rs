use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct ErrorResponse {
    pub reason: String,
}
