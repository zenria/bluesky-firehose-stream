# bluesky-firehose-stream

Stream and parse bluesky firehose messages.

This crate is heavily inspired from [`skystreamer`](https://github.com/FyraLabs/skystreamer). Some
very small amount of code has been partially copied and adapted from the original project (types.rs and subscription.rs).

Contrary to `skystreamer`, firehose message frames are fully decoded in [`atrium`](https://github.com/sugyan/atrium)
types, without further conversion.

Unknown message kinds are also decoded as an [`Ipld`](https://docs.rs/ipld-core/0.4.1/ipld_core/ipld/enum.Ipld.html)
enum allowing this create to handle even the more exotic events.

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
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
    // something with the decoded message here 🚀
}
```

## License

Licensed under either of

* Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
* MIT license
   ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
