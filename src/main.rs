#![allow(unused, dead_code)]
use std::io::Cursor;

use anyhow::Context;
use atrium_api::{
    app::bsky,
    com::atproto::sync::subscribe_repos::{Account, Commit, Handle, Identity, Tombstone},
    record,
    types::{CidLink, Collection as _},
};
use bluesky_firehose_stream::FirehoseMessage;
use cid::Cid;
use rs_car_sync::CarReader;
use skystreamer::{
    types::{commit::Record, CidOld, Frame, Subscription},
    RepoSubscription,
};
use tracing::{error, info, warn};
use tracing_subscriber::{fmt::SubscriberBuilder, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Init .env resolution
    let _ = dotenvy::dotenv();

    // Init logging
    SubscriberBuilder::default()
        // only enable colored output on real terminals
        .with_ansi(atty::is(atty::Stream::Stdout))
        .with_env_filter(EnvFilter::from_default_env())
        // build but do not install the subscriber.
        .finish()
        .init();

    loop {
        let mut subscription = RepoSubscription::new("bsky.network").await.unwrap();

        info!("Connected to firehose, let's stream");

        let timeout_duration = tokio::time::Duration::from_secs(30);

        while let Ok(Some(frame)) =
            tokio::time::timeout(timeout_duration, subscription.next()).await
        {
            let frame = match frame {
                Ok(f) => f,
                Err(e) => {
                    warn!("Unable to decode frame, skipping it! {e}");
                    continue;
                }
            };
            if let Err(e) = handle_frame(frame) {
                error!("Unable to handle frame: {e}");
            }
        }
        error!("Timeout occured, reconnecting...");
    }
}
fn handle_frame(frame: Frame) -> Result<(), bluesky_firehose_stream::Error> {
    let message = FirehoseMessage::try_from(frame)?;

    // for now do nothing
    if let FirehoseMessage::Identity(identity) = &message {
        println!("{}", serde_json::to_string(&identity).unwrap())
    }
    if let FirehoseMessage::Commit {
        did,
        operations,
        rev,
        time,
        commit,
    } = &message
    {
        //println!("{}", serde_json::to_string(&message).unwrap())
        if operations.len() == 1 {
            println!("{}", serde_json::to_string(&message).unwrap())
        }
    }
    Ok(())
}
