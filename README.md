# bluesky-firehose-stream

Stream and parse bluesky firehose messages.

This crate is heavily inspired from [`skystreamer`](https://github.com/FyraLabs/skystreamer). Some
very small amount of code has been partially copied and adapted from the original project (types.rs and subscription.rs).

Contrary to `skystreamer`, firehose message frames are fully decoded into [`atrium`](https://github.com/sugyan/atrium)
types, without further conversion.

Messages with unknown kind are also decoded as an [`Ipld`](https://docs.rs/ipld-core/0.4.1/ipld_core/ipld/enum.Ipld.html)
enum allowing this crate to handle even the more exotic events.

I'm currently running `bluesky-prometheus-exporter` for a month or so without any crash and only a handful of errors per days (invalid frames).

```rust
#[tokio::main]
async fn main() {
    loop {
        let mut subscription = match RepoSubscription::new("bsky.network").await {
            Ok(sub) => sub,
            Err(e) => {
                error!("Connecting to the bluesky firehose, {e}");
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
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
    // do something with the decoded message here 🚀
}
```

## Run examples

```bash
cargo run --example bluesky-prometheus-exporter --features examples
```

## TLS backend

By default, this crate depends on `native-tls` for handling TLS when connecting to the fireshose. To switch to `rustls` backend,
use `rustls-tls-native-roots` or `rustls-tls-webpki-roots` features.

eg.

```toml
bluesky-firehose-stream = { version="*", features = [
    "websocket",
    "rustls-tls-native-roots",
], default-features = false }
```

## License

Licensed under MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, shall be MIT licensed as above, without any additional terms or conditions.
