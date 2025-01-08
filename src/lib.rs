use std::{convert::Infallible, io::Cursor};

use atrium_api::{
    app::bsky::{self},
    com::atproto::sync::subscribe_repos::{Account, Commit, Handle, Identity, Tombstone},
    types::{
        string::{Datetime, Did},
        Collection as _,
    },
};
use cid::Cid;
use multihash::Multihash;
use rs_car_sync::CarDecodeError;
use serde::Serialize;
use serde_ipld_dagcbor::DecodeError;
use tracing::{error, warn};

pub mod frame;
#[cfg(feature = "websocket")]
pub mod subscription;

#[cfg(feature = "prometheus")]
pub mod metrics;

#[derive(Serialize)]
#[serde(tag = "kind")]
#[non_exhaustive]
pub enum FirehoseMessage {
    #[serde(rename = "commit")]
    Commit {
        did: Did,
        rev: String,
        time: Datetime,
        operations: Vec<Operation>,
        #[serde(skip)]
        commit: Commit,
    },
    #[serde(rename = "handle")]
    Handle(Handle),
    #[serde(rename = "tombstone")]
    Tombstone(Tombstone),
    #[serde(rename = "identity")]
    Identity(Identity),
    #[serde(rename = "account")]
    Account(Account),
}

impl FirehoseMessage {
    pub fn kind(&self) -> FirehoseMessageKind {
        match self {
            FirehoseMessage::Commit { .. } => FirehoseMessageKind::Commit,
            FirehoseMessage::Handle(_object) => FirehoseMessageKind::Handle,
            FirehoseMessage::Tombstone(_object) => FirehoseMessageKind::Tombstone,
            FirehoseMessage::Identity(_object) => FirehoseMessageKind::Identity,
            FirehoseMessage::Account(_object) => FirehoseMessageKind::Account,
        }
    }
}
#[derive(Serialize, Debug, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum FirehoseMessageKind {
    Commit,
    Handle,
    Tombstone,
    Identity,
    Account,
}
impl FirehoseMessageKind {
    pub fn as_str(&self) -> &str {
        match self {
            FirehoseMessageKind::Commit => "commit",
            FirehoseMessageKind::Handle => "handle",
            FirehoseMessageKind::Tombstone => "tombstone",
            FirehoseMessageKind::Identity => "identity",
            FirehoseMessageKind::Account => "account",
        }
    }
}
#[derive(Serialize, Debug)]
#[serde(untagged)]
#[non_exhaustive]
pub enum Record {
    Unknown(ipld_core::ipld::Ipld),
    Post(atrium_api::types::Object<bsky::feed::post::RecordData>),
    Follow(atrium_api::types::Object<bsky::graph::follow::RecordData>),
    Block(atrium_api::types::Object<bsky::graph::block::RecordData>),
    Repost(atrium_api::types::Object<bsky::feed::repost::RecordData>),
    Like(atrium_api::types::Object<bsky::feed::like::RecordData>),
    Listitem(atrium_api::types::Object<bsky::graph::listitem::RecordData>),
    Generator(atrium_api::types::Object<bsky::feed::generator::RecordData>),
    Profile(atrium_api::types::Object<bsky::actor::profile::RecordData>),
    List(atrium_api::types::Object<bsky::graph::list::RecordData>),
    Starterpack(atrium_api::types::Object<bsky::graph::starterpack::RecordData>),
}

#[derive(Serialize)]
#[serde(tag = "operation", rename_all = "lowercase")]
pub enum Operation {
    Create {
        #[serde(flatten)]
        operation_meta: OperationMeta,
        record: Record,
        cid: String,
    },
    Update {
        #[serde(flatten)]
        operation_meta: OperationMeta,
        record: Record,
        cid: String,
    },
    Delete(OperationMeta),
}
impl Operation {
    pub fn kind(&self) -> OperationKind {
        match self {
            Operation::Create { .. } => OperationKind::Create,
            Operation::Update { .. } => OperationKind::Update,
            Operation::Delete(_) => OperationKind::Delete,
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub enum OperationKind {
    Create,
    Update,
    Delete,
}
impl OperationKind {
    pub fn as_str(&self) -> &str {
        match self {
            OperationKind::Create => "create",
            OperationKind::Update => "update",
            OperationKind::Delete => "delete",
        }
    }
}
#[derive(Serialize, Debug)]
pub struct OperationMeta {
    pub collection: String,
    pub rkey: String,
}
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Unknown frame type {0}")]
    UnknownFrameType(String, crate::frame::MessageFrame),
    #[error("No type in frame")]
    NoTypeInFrame(crate::frame::MessageFrame),
    #[error("Error Frame")]
    FrameError(crate::frame::ErrorFrame),
    #[error("Frame decode error {0}")]
    DagCborDecodeError(DecodeError<Infallible>, crate::frame::MessageFrame),
    #[error("CAR decode error {0}")]
    CarDecodeError(CarDecodeError, Commit),
    #[error("No block found for commit {did:?} {rev} {operation} {path}")]
    NoBlockForCommit {
        operation: String,
        rev: String,
        did: Did,
        path: String,
    },
    #[error("Unknown commit operation `{operation}` {}/{}", operation_meta.collection, operation_meta.rkey)]
    UnknownCommitOperation {
        operation: String,
        operation_meta: OperationMeta,
        record: Record,
        cid: String,
    },
}

impl TryFrom<crate::frame::Frame> for FirehoseMessage {
    type Error = Error;

    fn try_from(frame: crate::frame::Frame) -> Result<Self, Self::Error> {
        match frame {
            crate::frame::Frame::Message(Some(t), message_frame) => match t.as_str() {
                "#commit" => {
                    let commit =
                        serde_ipld_dagcbor::from_slice::<Commit>(message_frame.body.as_slice())
                            .map_err(|e| Error::DagCborDecodeError(e, message_frame.clone()))?;

                    let mut block_reader = Cursor::new(&commit.blocks);
                    let (blocks, _) = rs_car_sync::car_read_all(&mut block_reader, true)
                        .map_err(|e| Error::CarDecodeError(e, commit.clone()))?;

                    let mut operations = Vec::new();

                    for op in &commit.ops {
                        let (nsid, rkey) = {
                            let mut split = op.path.split("/");
                            (split.next().unwrap(), split.next())
                        };
                        if op.action == "delete" {
                            operations.push(Operation::Delete(OperationMeta {
                                collection: nsid.to_string(),
                                rkey: rkey.unwrap_or_default().to_string(),
                            }));
                            continue;
                        }
                        let Some(op_cid_acid) = &op.cid else {
                            if op.action != "delete" {
                                warn!("No block cid for op {} {}", op.action, op.path);
                            } else {
                            }
                            continue;
                        };
                        let op_cid = op_cid_acid.0;

                        let record = match blocks.iter().find(|(cid, _data)| {
                            let cid: Cid = CidOld::from(*cid).try_into().unwrap();
                            cid == op_cid
                        }) {
                            Some(block) => match nsid {
                                bsky::feed::Post::NSID => {
                                    Record::Post(serde_ipld_dagcbor::from_slice(&block.1).map_err(
                                        |e| Error::DagCborDecodeError(e, message_frame.clone()),
                                    )?)
                                }
                                bsky::graph::Follow::NSID => Record::Follow(
                                    serde_ipld_dagcbor::from_slice(&block.1).map_err(|e| {
                                        Error::DagCborDecodeError(e, message_frame.clone())
                                    })?,
                                ),
                                bsky::graph::Block::NSID => Record::Block(
                                    serde_ipld_dagcbor::from_slice(&block.1).map_err(|e| {
                                        Error::DagCborDecodeError(e, message_frame.clone())
                                    })?,
                                ),
                                bsky::feed::Repost::NSID => Record::Repost(
                                    serde_ipld_dagcbor::from_slice(&block.1).map_err(|e| {
                                        Error::DagCborDecodeError(e, message_frame.clone())
                                    })?,
                                ),
                                bsky::feed::Like::NSID => {
                                    Record::Like(serde_ipld_dagcbor::from_slice(&block.1).map_err(
                                        |e| Error::DagCborDecodeError(e, message_frame.clone()),
                                    )?)
                                }
                                bsky::graph::Listitem::NSID => Record::Listitem(
                                    serde_ipld_dagcbor::from_slice(&block.1).map_err(|e| {
                                        Error::DagCborDecodeError(e, message_frame.clone())
                                    })?,
                                ),
                                bsky::feed::Generator::NSID => Record::Generator(
                                    serde_ipld_dagcbor::from_slice(&block.1).map_err(|e| {
                                        Error::DagCborDecodeError(e, message_frame.clone())
                                    })?,
                                ),
                                bsky::actor::Profile::NSID => Record::Profile(
                                    serde_ipld_dagcbor::from_slice(&block.1).map_err(|e| {
                                        Error::DagCborDecodeError(e, message_frame.clone())
                                    })?,
                                ),
                                bsky::graph::List::NSID => {
                                    Record::List(serde_ipld_dagcbor::from_slice(&block.1).map_err(
                                        |e| Error::DagCborDecodeError(e, message_frame.clone()),
                                    )?)
                                }
                                bsky::graph::Starterpack::NSID => Record::Starterpack(
                                    serde_ipld_dagcbor::from_slice(&block.1).map_err(|e| {
                                        Error::DagCborDecodeError(e, message_frame.clone())
                                    })?,
                                ),

                                _ => Record::Unknown(
                                    serde_ipld_dagcbor::from_slice::<ipld_core::ipld::Ipld>(
                                        &block.1,
                                    )
                                    .map_err(|e| {
                                        Error::DagCborDecodeError(e, message_frame.clone())
                                    })?,
                                ),
                            },
                            None => Err(Error::NoBlockForCommit {
                                operation: op.action.clone(),
                                rev: commit.rev.clone(),
                                did: commit.repo.clone(),
                                path: op.path.clone(),
                            })?,
                        };
                        let operation = match op.action.as_str() {
                            "create" => Operation::Create {
                                operation_meta: OperationMeta {
                                    collection: nsid.to_string(),
                                    rkey: rkey.unwrap_or_default().to_string(),
                                },
                                record,
                                cid: op_cid.to_string(),
                            },
                            "update" => Operation::Update {
                                operation_meta: OperationMeta {
                                    collection: nsid.to_string(),
                                    rkey: rkey.unwrap_or_default().to_string(),
                                },
                                record,
                                cid: op_cid.to_string(),
                            },
                            other => Err(Error::UnknownCommitOperation {
                                operation: other.to_string(),
                                operation_meta: OperationMeta {
                                    collection: nsid.to_string(),
                                    rkey: rkey.unwrap_or_default().to_string(),
                                },
                                record,
                                cid: op_cid.to_string(),
                            })?,
                        };
                        operations.push(operation);
                    }
                    Ok(FirehoseMessage::Commit {
                        operations,
                        rev: commit.rev.clone(),
                        time: commit.time.clone(),
                        did: commit.repo.clone(),
                        commit,
                    })
                }
                "#account" => Ok(FirehoseMessage::Account(
                    serde_ipld_dagcbor::from_slice(message_frame.body.as_slice())
                        .map_err(|e| Error::DagCborDecodeError(e, message_frame))?,
                )),
                "#handle" => Ok(FirehoseMessage::Handle(
                    serde_ipld_dagcbor::from_slice(message_frame.body.as_slice())
                        .map_err(|e| Error::DagCborDecodeError(e, message_frame))?,
                )),
                "#tombstone" => Ok(FirehoseMessage::Tombstone(
                    serde_ipld_dagcbor::from_slice(message_frame.body.as_slice())
                        .map_err(|e| Error::DagCborDecodeError(e, message_frame))?,
                )),
                "#identity" => Ok(FirehoseMessage::Identity(
                    serde_ipld_dagcbor::from_slice(message_frame.body.as_slice())
                        .map_err(|e| Error::DagCborDecodeError(e, message_frame))?,
                )),
                t => Err(Error::UnknownFrameType(t.to_string(), message_frame))?,
            },
            crate::frame::Frame::Message(None, message_frame) => {
                Err(Error::NoTypeInFrame(message_frame))
            }
            crate::frame::Frame::Error(error_frame) => Err(Error::FrameError(error_frame)),
        }
    }
}

pub struct CidOld(cid_old::Cid);

impl From<cid_old::Cid> for CidOld {
    fn from(value: cid_old::Cid) -> Self {
        Self(value)
    }
}
impl TryFrom<CidOld> for Cid {
    type Error = cid::Error;
    fn try_from(value: CidOld) -> std::result::Result<Self, Self::Error> {
        let version = match value.0.version() {
            cid_old::Version::V0 => cid::Version::V0,
            cid_old::Version::V1 => cid::Version::V1,
        };

        let codec = value.0.codec();
        let hash = value.0.hash();
        let hash = Multihash::from_bytes(&hash.to_bytes())?;

        Self::new(version, codec, hash)
    }
}
