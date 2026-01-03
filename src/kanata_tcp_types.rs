use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug)]
pub struct KanataServerResponse {
    pub status: String,
    pub msg: Option<String>,
}

#[derive(Serialize, Debug)]
pub enum KanataClientMessage {
    ChangeLayer { new: String },
    ReloadNum { index: usize },
}
