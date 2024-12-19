pub mod types;

use std::convert::Infallible;
pub const BLUESKY_FEED_DOMAIN: &str = "bsky.network";
use atrium_api::{com::atproto::sync::subscribe_repos::NSID, types::CidLink};

use futures::StreamExt;

use ipld_core::ipld::Ipld;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
    MaybeTlsStream, WebSocketStream,
};
pub use types::Frame;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed to connect to websocket: {0}")]
    Connect(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("Failed to decide CBOR: {0}")]
    CborDecoder(#[from] serde_ipld_dagcbor::DecodeError<std::io::Error>),
    #[error("Failed to decode CBOR (How!?): {0}")]
    CborDecode(#[from] serde_ipld_dagcbor::DecodeError<Infallible>),
    #[error("Failed to decode CAR data: {0}")]
    CarDecoder(#[from] rs_car_sync::CarDecodeError),
    #[error("Could not find item with operation cid {0:?} out of {1} items")]
    ItemNotFound(Option<CidLink>, usize),
    #[error("Invalid frame data: {0:?}")]
    InvalidFrameData(Vec<u8>),
    #[error("Invalid frame type: {0:?}")]
    InvalidFrameType(Ipld),
    #[error("ATrium error: {0}")]
    AtriumError(String),
}

pub(crate) type Result<T> = std::result::Result<T, Error>;

pub struct RepoSubscription {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl RepoSubscription {
    pub async fn new(bgs: &str) -> std::result::Result<Self, Error> {
        // todo: somehow get the websocket to update the damn params
        let request = format!("wss://{bgs}/xrpc/{NSID}").into_client_request()?;
        // request.
        let (stream, res) = connect_async(request).await?;
        tracing::debug!("Connected to websocket: {:?}", res);
        Ok(RepoSubscription { stream })
    }
    pub async fn next(&mut self) -> Option<Result<Frame>> {
        if let Some(Ok(Message::Binary(data))) = self.stream.next().await {
            #[cfg(feature = "prometheus")]
            {
                metrics::FIREHOSE_BYTE_COUNTER.inc_by(data.len() as u64);
            }
            Some(Frame::try_from(data.as_slice()))
        } else {
            None
        }
    }
}

#[cfg(feature = "prometheus")]
mod metrics {
    use lazy_static::lazy_static;
    use prometheus::IntCounter;

    lazy_static! {
        pub(crate) static ref FIREHOSE_BYTE_COUNTER: IntCounter = crate::metrics::create_counter(
            "bluesky_firehose_streamer_bytes_in",
            "Input bytes from bluesky firehose"
        );
    }
}
