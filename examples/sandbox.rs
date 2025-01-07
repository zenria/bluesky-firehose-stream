use atrium_api::{
    app::bsky::{
        feed::post::{RecordEmbedRefs, RecordLabelsRefs},
        richtext::facet::MainFeaturesItem,
    },
    types::Union,
};
use bluesky_firehose_stream::{
    subscription::{Frame, RepoSubscription},
    FirehoseMessage, Operation, Record,
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
        error!("Timeout occurred, reconnecting...");
    }
}
fn handle_frame(frame: Frame) -> Result<(), bluesky_firehose_stream::Error> {
    let message = FirehoseMessage::try_from(frame)?;

    // for now do nothing
    if let FirehoseMessage::Identity(_identity) = &message {
        //println!("{}", serde_json::to_string(&identity).unwrap())
    }
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
                operation_meta: _,
                record,
                cid: _,
            } = op
            {
                if let Record::Starterpack(pack) = record {
                    println!("{}", serde_json::to_string(&pack).unwrap());
                }
                if let Record::Post(post) = record {
                    let labels = match &post.labels {
                        Some(Union::Refs(RecordLabelsRefs::ComAtprotoLabelDefsSelfLabels(
                            labels,
                        ))) => labels
                            .data
                            .values
                            .iter()
                            .map(|label| label.val.clone())
                            .collect::<Vec<_>>(),
                        Some(Union::Unknown(unknown)) => {
                            error!("Unknown Labels {}", serde_json::to_string(unknown).unwrap());
                            vec![]
                        }
                        None => {
                            vec![]
                        }
                    };
                    let mut links = post
                        .facets
                        .as_ref()
                        .map(|facets| {
                            facets
                                .iter()
                                .flat_map(|facet| {
                                    facet.features.iter().filter_map(|feature| match feature {
                                        Union::Refs(MainFeaturesItem::Link(link)) => {
                                            Some(link.uri.clone())
                                        }
                                        _ => None,
                                    })
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    if let Some(Union::Refs(RecordEmbedRefs::AppBskyEmbedExternalMain(ext_embed))) =
                        &post.embed
                    {
                        if !links.contains(&ext_embed.external.uri) {
                            links.push(ext_embed.external.uri.clone());
                        }
                    }
                    if !links.is_empty() {
                        println!("{links:?} {labels:?}");
                    }
                }
            }
        }
    }
    Ok(())
}
