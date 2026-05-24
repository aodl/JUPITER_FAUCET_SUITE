use pocket_ic::{PocketIcBuilder, StartServerParams, start_server};
use std::sync::OnceLock;
use std::time::Duration;

const SERVER_HARD_TTL_SECS: u64 = 3 * 60 * 60;

static SERVER_URL: OnceLock<String> = OnceLock::new();

pub fn builder() -> PocketIcBuilder {
    let server_url = SERVER_URL.get_or_init(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("failed to create PocketIC server runtime");
        let (_, url) = runtime.block_on(start_server(StartServerParams {
            reuse: true,
            hard_ttl: Some(Duration::from_secs(SERVER_HARD_TTL_SECS)),
            ..Default::default()
        }));
        url.to_string()
    });

    PocketIcBuilder::new().with_server_url(
        server_url
            .parse()
            .expect("PocketIC server URL from start_server should parse"),
    )
}
