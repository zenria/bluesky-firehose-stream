use std::{net::SocketAddr, time::Duration};

use anyhow::Context;
use axum::{
    routing::{any, get},
    Router,
};
use bluesky_firehose_stream::{
    metrics::create_counter_with_labels,
    subscription::{Frame, RepoSubscription},
    FirehoseMessage, Operation,
};
use lazy_static::lazy_static;
use prometheus::{Encoder, IntCounterVec, TextEncoder};
use reqwest::StatusCode;
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

    tokio::spawn(firehose_consume_loop());

    let routes = Router::new()
        .route("/metrics", get(|| async { generate_metrics() }))
        .route("/health", any(|| async { "OK" }))
        .route("/", get(|| async { "Go to /metrics or /health" }))
        .route("/favicon.ico", any(|| async { StatusCode::NOT_FOUND }));

    tracing::info!("Starting HTTP server at 0.0.0.0:8956");

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8956")
        .await
        .context(format!("Unable to listen to 0.0.0.0:8956",))?;

    Ok(axum::serve(
        listener,
        routes.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .context("Unable to start axum http server")?)
}
pub fn generate_metrics() -> String {
    // Gather the metrics.
    let mut buffer = vec![];
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    encoder.encode(&metric_families, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
}
async fn firehose_consume_loop() {
    loop {
        info!("Connecting to the bluesky firehose, let's stream");

        let mut subscription = match RepoSubscription::new("bsky.network").await {
            Ok(sub) => sub,
            Err(e) => {
                error!("Connecting to the bluesky firehose, {e}");
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        info!("Connected to the bluesky firehose, let's stream");

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
    FIREHOSE_FRAME_COUNTER
        .with_label_values(&[message.kind().as_str()])
        .inc();

    if let FirehoseMessage::Commit {
        did: _,
        operations,
        rev: _,
        time: _,
        commit: _,
    } = &message
    {
        for op in operations {
            if let Operation::Create {
                operation_meta,
                record: _,
                cid: _,
            } = op
            {
                FIREHOSE_COMMIT_COUNTER
                    .with_label_values(&[op.kind().as_str(), operation_meta.collection.as_str()])
                    .inc();
            }
        }
    }
    Ok(())
}

lazy_static! {
    static ref FIREHOSE_FRAME_COUNTER: IntCounterVec = create_counter_with_labels(
        "bluesky_firehose_streamer_frames_in",
        "Input frames from bluesky firehose",
        &["kind"]
    );
    static ref FIREHOSE_COMMIT_COUNTER: IntCounterVec = create_counter_with_labels(
        "bluesky_firehose_streamer_commits_in",
        "Input frames from bluesky firehose",
        &["operation", "collection"]
    );
}
