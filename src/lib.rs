use std::{convert::Infallible, io::Cursor};

use atrium_api::{
    app::bsky,
    com::atproto::sync::subscribe_repos::{Account, Commit, Handle, Identity, Tombstone},
    types::{
        string::{Datetime, Did},
        Collection as _,
    },
};
use cid::Cid;
use rs_car_sync::CarDecodeError;
use serde::Serialize;
use serde_ipld_dagcbor::DecodeError;
use skystreamer::types::CidOld;
use tracing::{error, warn};

#[derive(Serialize)]
#[serde(tag = "kind")]
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

#[derive(Serialize)]
#[serde(tag = "type")]
pub enum Record {
    Unknown(serde_json::Value),
    Post(atrium_api::types::Object<bsky::feed::post::RecordData>),
    Follow(atrium_api::types::Object<bsky::graph::follow::RecordData>),
    Block(atrium_api::types::Object<bsky::graph::block::RecordData>),
    Repost(atrium_api::types::Object<bsky::feed::repost::RecordData>),
    Like(atrium_api::types::Object<bsky::feed::like::RecordData>),
    Listitem(atrium_api::types::Object<bsky::graph::listitem::RecordData>),
    Generator(atrium_api::types::Object<bsky::feed::generator::RecordData>),
    Profile(atrium_api::types::Object<bsky::actor::profile::RecordData>),
    List(atrium_api::types::Object<bsky::graph::list::RecordData>),
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
    Delete(OperationMeta),
}

#[derive(Serialize)]
pub struct OperationMeta {
    pub collection: String,
    pub rkey: String,
}
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Unknown frame type {0}")]
    UnknownFrameType(String, skystreamer::types::MessageFrame),
    #[error("No type in frame")]
    NoTypeInFrame(skystreamer::types::MessageFrame),
    #[error("Error Frame")]
    FrameError(skystreamer::types::ErrorFrame),
    #[error("Frame decode error {0}")]
    DagCborDecodeError(DecodeError<Infallible>, skystreamer::types::MessageFrame),
    #[error("CAR decode error {0}")]
    CarDecodeError(CarDecodeError, Commit),
    #[error("No block found for commit {did:?} {rev} {operation} {path}")]
    NoBlockForCommit {
        operation: String,
        rev: String,
        did: Did,
        path: String,
    },
}

impl TryFrom<skystreamer::types::Frame> for FirehoseMessage {
    type Error = Error;

    fn try_from(frame: skystreamer::types::Frame) -> Result<Self, Self::Error> {
        match frame {
            skystreamer::types::Frame::Message(Some(t), message_frame) => match t.as_str() {
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
                                bsky::feed::Post::NSID => Record::Post(
                                    serde_ipld_dagcbor::from_slice::<
                                        atrium_api::app::bsky::feed::post::Record,
                                    >(&block.1)
                                    .map_err(|e| {
                                        Error::DagCborDecodeError(e, message_frame.clone())
                                    })?,
                                ),
                                bsky::graph::Follow::NSID => Record::Follow(
                                    serde_ipld_dagcbor::from_slice::<
                                        atrium_api::app::bsky::graph::follow::Record,
                                    >(&block.1)
                                    .map_err(|e| {
                                        Error::DagCborDecodeError(e, message_frame.clone())
                                    })?,
                                ),
                                bsky::graph::Block::NSID => Record::Block(
                                    serde_ipld_dagcbor::from_slice::<
                                        atrium_api::app::bsky::graph::block::Record,
                                    >(&block.1)
                                    .map_err(|e| {
                                        Error::DagCborDecodeError(e, message_frame.clone())
                                    })?,
                                ),
                                bsky::feed::Repost::NSID => Record::Repost(
                                    serde_ipld_dagcbor::from_slice::<
                                        atrium_api::app::bsky::feed::repost::Record,
                                    >(&block.1)
                                    .map_err(|e| {
                                        Error::DagCborDecodeError(e, message_frame.clone())
                                    })?,
                                ),
                                bsky::feed::Like::NSID => Record::Like(
                                    serde_ipld_dagcbor::from_slice::<
                                        atrium_api::app::bsky::feed::like::Record,
                                    >(&block.1)
                                    .map_err(|e| {
                                        Error::DagCborDecodeError(e, message_frame.clone())
                                    })?,
                                ),
                                bsky::graph::Listitem::NSID => Record::Listitem(
                                    serde_ipld_dagcbor::from_slice::<
                                        atrium_api::app::bsky::graph::listitem::Record,
                                    >(&block.1)
                                    .map_err(|e| {
                                        Error::DagCborDecodeError(e, message_frame.clone())
                                    })?,
                                ),
                                bsky::feed::Generator::NSID => Record::Generator(
                                    serde_ipld_dagcbor::from_slice::<
                                        atrium_api::app::bsky::feed::generator::Record,
                                    >(&block.1)
                                    .map_err(|e| {
                                        Error::DagCborDecodeError(e, message_frame.clone())
                                    })?,
                                ),
                                bsky::actor::Profile::NSID => Record::Profile(
                                    serde_ipld_dagcbor::from_slice::<
                                        atrium_api::app::bsky::actor::profile::Record,
                                    >(&block.1)
                                    .map_err(|e| {
                                        Error::DagCborDecodeError(e, message_frame.clone())
                                    })?,
                                ),
                                bsky::graph::List::NSID => Record::List(
                                    serde_ipld_dagcbor::from_slice::<
                                        atrium_api::app::bsky::graph::list::Record,
                                    >(&block.1)
                                    .map_err(|e| {
                                        Error::DagCborDecodeError(e, message_frame.clone())
                                    })?,
                                ),

                                _ => Record::Unknown(
                                    serde_ipld_dagcbor::from_slice::<serde_json::Value>(&block.1)
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
                        operations.push(Operation::Create {
                            operation_meta: OperationMeta {
                                collection: nsid.to_string(),
                                rkey: rkey.unwrap_or_default().to_string(),
                            },
                            record,
                            cid: op_cid.to_string(),
                        });
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
            skystreamer::types::Frame::Message(None, message_frame) => {
                Err(Error::NoTypeInFrame(message_frame))
            }
            skystreamer::types::Frame::Error(error_frame) => Err(Error::FrameError(error_frame)),
        }
    }
}
