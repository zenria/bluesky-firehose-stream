use atrium_api::{
    app::bsky::feed::post::{RecordEmbedRefs, RecordLabelsRefs},
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
        for op in operations {
            if let Operation::Create {
                operation_meta,
                record,
                cid,
            } = op
            {
                if let Record::Post(post) = record {
                    if let Some(Union::Refs(RecordEmbedRefs::AppBskyEmbedExternalMain(external))) =
                        &post.data.embed
                    {
                        //println!("external uri {}", external.external.uri);
                    }
                    if let Some(Union::Refs(RecordLabelsRefs::ComAtprotoLabelDefsSelfLabels(
                        labels,
                    ))) = &post.labels
                    {
                        if labels.data.values.len() > 0 {
                            //println!("{:?}", labels.data.values);
                        }
                    }
                    /*  println!(
                        "{} {} {}",
                        did.as_str(),
                        operation_meta.rkey,
                        serde_json::to_string(&post).unwrap()
                    );*/
                }
            }
        }
    }
    Ok(())
}
