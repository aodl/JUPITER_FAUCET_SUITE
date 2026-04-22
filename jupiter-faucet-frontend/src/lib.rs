use ic_asset_certification::{
    Asset, AssetConfig, AssetEncoding, AssetFallbackConfig, AssetMap, AssetRouter,
};
use ic_cdk::{
    api::{canister_cycle_balance, certified_data_set, data_certificate},
    init, post_upgrade, query,
};
use ic_http_certification::{
    utils::add_v2_certificate_header, DefaultCelBuilder, DefaultResponseCertification,
    DefaultResponseOnlyCelExpression,
    HeaderField, HttpCertification, HttpCertificationPath, HttpCertificationTree,
    HttpCertificationTreeEntry, HttpRequest, HttpResponse, StatusCode,
    CERTIFICATE_EXPRESSION_HEADER_NAME,
};
use include_dir::{include_dir, Dir};
use serde::Serialize;
use std::{cell::RefCell, rc::Rc, time::Duration};

#[derive(Debug, Clone, Serialize)]
pub struct Metrics {
    pub num_assets: usize,
    pub num_fallback_assets: usize,
    pub cycle_balance: u128,
}

#[derive(Debug, Clone)]
struct CertifiedMetricsSnapshot {
    body: Vec<u8>,
    headers: Vec<HeaderField>,
    cel_expr: DefaultResponseOnlyCelExpression<'static>,
}

const METRICS_REFRESH_INTERVAL_SECS: u64 = 60;

fn initialize_runtime_state() {
    certify_all_assets();
    refresh_certified_metrics_snapshot();
    install_metrics_refresh_timer();
}

#[init]
fn init() {
    initialize_runtime_state();
}

#[post_upgrade]
fn post_upgrade() {
    initialize_runtime_state();
}

#[query]
fn http_request(req: HttpRequest) -> HttpResponse {
    let path = match req.get_path() {
        Ok(path) => path,
        Err(_) => return plain_error_response(StatusCode::BAD_REQUEST, "bad request path"),
    };

    match request_target_for_path(&path) {
        RequestTarget::Metrics => serve_metrics(),
        RequestTarget::Asset => serve_asset(&req),
    }
}

thread_local! {
    static HTTP_TREE: Rc<RefCell<HttpCertificationTree>> = Default::default();
    static ASSET_ROUTER: RefCell<AssetRouter<'static>> = RefCell::new(
        AssetRouter::with_tree(HTTP_TREE.with(|tree| tree.clone()))
    );
    static CERTIFIED_METRICS_SNAPSHOT: RefCell<Option<CertifiedMetricsSnapshot>> = const { RefCell::new(None) };
}

static ASSETS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/assets");
const IMMUTABLE_ASSET_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";
const NO_CACHE_ASSET_CACHE_CONTROL: &str = "public, no-cache, no-store";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestTarget {
    Metrics,
    Asset,
}

fn request_target_for_path(path: &str) -> RequestTarget {
    if path == "/metrics" {
        RequestTarget::Metrics
    } else {
        RequestTarget::Asset
    }
}

fn plain_error_response(status: StatusCode, message: &str) -> HttpResponse<'static> {
    HttpResponse::builder()
        .with_status_code(status)
        .with_body(message.as_bytes().to_vec())
        .with_headers(get_asset_headers(vec![
            ("content-type".to_string(), "text/plain; charset=utf-8".to_string()),
            ("cache-control".to_string(), NO_CACHE_ASSET_CACHE_CONTROL.to_string()),
        ]))
        .build()
}

fn collect_assets<'content, 'path>(
    dir: &'content Dir<'path>,
    assets: &mut Vec<Asset<'content, 'path>>,
) {
    for file in dir.files() {
        assets.push(Asset::new(file.path().to_string_lossy(), file.contents()));
    }

    for dir in dir.dirs() {
        collect_assets(dir, assets);
    }
}

fn metrics_tree_path() -> HttpCertificationPath<'static> {
    HttpCertificationPath::exact("/metrics")
}

fn metrics_cel_expression() -> DefaultResponseOnlyCelExpression<'static> {
    DefaultCelBuilder::response_only_certification()
        .with_response_certification(DefaultResponseCertification::response_header_exclusions(vec![]))
        .build()
}

fn build_certified_metrics_snapshot(asset_router: &AssetRouter<'static>) -> CertifiedMetricsSnapshot {
    let metrics = Metrics {
        num_assets: asset_router.get_assets().len(),
        num_fallback_assets: asset_router.get_fallback_assets().len(),
        cycle_balance: canister_cycle_balance(),
    };
    let body = serde_json::to_vec(&metrics).expect("failed to serialize metrics");
    let cel_expr = metrics_cel_expression();
    let headers = get_asset_headers(vec![
        (CERTIFICATE_EXPRESSION_HEADER_NAME.to_string(), cel_expr.to_string()),
        ("content-type".to_string(), "application/json".to_string()),
        ("cache-control".to_string(), NO_CACHE_ASSET_CACHE_CONTROL.to_string()),
    ]);
    CertifiedMetricsSnapshot { body, headers, cel_expr }
}

fn install_metrics_refresh_timer() {
    ic_cdk_timers::set_timer_interval(Duration::from_secs(METRICS_REFRESH_INTERVAL_SECS), || async {
        refresh_certified_metrics_snapshot();
    });
}

fn refresh_certified_metrics_snapshot() {
    ASSET_ROUTER.with_borrow(|asset_router| {
        let snapshot = build_certified_metrics_snapshot(asset_router);
        let response = HttpResponse::builder()
            .with_status_code(StatusCode::OK)
            .with_body(snapshot.body.clone())
            .with_headers(snapshot.headers.clone())
            .build();
        let certification = HttpCertification::response_only(&snapshot.cel_expr, &response, None)
            .expect("failed to certify metrics response");
        let metrics_tree_path = metrics_tree_path();
        let metrics_tree_entry = HttpCertificationTreeEntry::new(&metrics_tree_path, &certification);

        HTTP_TREE.with(|tree| {
            let mut tree = tree.borrow_mut();
            tree.delete_by_path(&metrics_tree_path);
            tree.insert(&metrics_tree_entry);
            certified_data_set(&tree.root_hash());
        });

        CERTIFIED_METRICS_SNAPSHOT.with(|stored| *stored.borrow_mut() = Some(snapshot));
    });
}

fn certify_all_assets() {
    let compressed_encodings = vec![
        AssetEncoding::Brotli.default_config(),
        AssetEncoding::Gzip.default_config(),
    ];

    let asset_configs = vec![
        AssetConfig::File {
            path: "index.html".to_string(),
            content_type: Some("text/html".to_string()),
            headers: get_asset_headers(vec![(
                "cache-control".to_string(),
                NO_CACHE_ASSET_CACHE_CONTROL.to_string(),
            )]),
            fallback_for: vec![AssetFallbackConfig {
                scope: "/".to_string(),
                status_code: Some(StatusCode::OK),
            }],
            aliased_by: vec!["/".to_string()],
            encodings: compressed_encodings.clone(),
        },
        AssetConfig::Pattern {
            pattern: "**/*.js".to_string(),
            content_type: Some("text/javascript".to_string()),
            headers: get_asset_headers(vec![(
                "cache-control".to_string(),
                IMMUTABLE_ASSET_CACHE_CONTROL.to_string(),
            )]),
            encodings: compressed_encodings.clone(),
        },
        AssetConfig::Pattern {
            pattern: "**/*.css".to_string(),
            content_type: Some("text/css".to_string()),
            headers: get_asset_headers(vec![(
                "cache-control".to_string(),
                IMMUTABLE_ASSET_CACHE_CONTROL.to_string(),
            )]),
            encodings: compressed_encodings,
        },
        AssetConfig::Pattern {
            pattern: "**/*.ico".to_string(),
            content_type: Some("image/x-icon".to_string()),
            headers: get_asset_headers(vec![(
                "cache-control".to_string(),
                IMMUTABLE_ASSET_CACHE_CONTROL.to_string(),
            )]),
            encodings: vec![],
        },
        AssetConfig::Pattern {
            pattern: "**/*.{png,jpg,jpeg}".to_string(),
            content_type: None,
            headers: get_asset_headers(vec![(
                "cache-control".to_string(),
                IMMUTABLE_ASSET_CACHE_CONTROL.to_string(),
            )]),
            encodings: vec![],
        },
        AssetConfig::Pattern {
            pattern: "**/*.webp".to_string(),
            content_type: Some("image/webp".to_string()),
            headers: get_asset_headers(vec![(
                "cache-control".to_string(),
                IMMUTABLE_ASSET_CACHE_CONTROL.to_string(),
            )]),
            encodings: vec![],
        },
        AssetConfig::Pattern {
            pattern: "**/*.svg".to_string(),
            content_type: Some("image/svg+xml".to_string()),
            headers: get_asset_headers(vec![(
                "cache-control".to_string(),
                IMMUTABLE_ASSET_CACHE_CONTROL.to_string(),
            )]),
            encodings: vec![],
        },
        AssetConfig::File {
            path: ".well-known/ic-domains".to_string(),
            content_type: Some("text/plain".to_string()),
            headers: get_asset_headers(vec![(
                "cache-control".to_string(),
                NO_CACHE_ASSET_CACHE_CONTROL.to_string(),
            )]),
            fallback_for: vec![],
            aliased_by: vec![],
            encodings: vec![],
        },
    ];

    let mut assets = Vec::new();
    collect_assets(&ASSETS_DIR, &mut assets);

    ASSET_ROUTER.with_borrow_mut(|asset_router| {
        if let Err(err) = asset_router.certify_assets(assets, asset_configs) {
            ic_cdk::trap(&format!("failed to certify frontend assets: {err}"));
        }

        certified_data_set(&asset_router.root_hash());
    });
}

fn serve_metrics() -> HttpResponse<'static> {
    let Some(snapshot) = CERTIFIED_METRICS_SNAPSHOT.with(|stored| stored.borrow().clone()) else {
        return plain_error_response(StatusCode::INTERNAL_SERVER_ERROR, "metrics snapshot unavailable");
    };

    let mut response = HttpResponse::builder()
        .with_status_code(StatusCode::OK)
        .with_body(snapshot.body)
        .with_headers(snapshot.headers)
        .build();

    let Some(certificate) = data_certificate() else {
        return plain_error_response(StatusCode::INTERNAL_SERVER_ERROR, "certificate unavailable");
    };

    HTTP_TREE.with(|tree| {
        let tree = tree.borrow();
        let metrics_tree_path = metrics_tree_path();
        let certification = match HttpCertification::response_only(&snapshot.cel_expr, &response, None) {
            Ok(certification) => certification,
            Err(_) => {
                return plain_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "metrics certification unavailable",
                )
            }
        };
        let metrics_tree_entry = HttpCertificationTreeEntry::new(&metrics_tree_path, &certification);
        let witness = match tree.witness(&metrics_tree_entry, "/metrics") {
            Ok(witness) => witness,
            Err(_) => {
                return plain_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "metrics witness unavailable",
                )
            }
        };
        add_v2_certificate_header(
            &certificate,
            &mut response,
            &witness,
            &metrics_tree_path.to_expr_path(),
        );

        response
    })
}

fn serve_asset(req: &HttpRequest) -> HttpResponse<'static> {
    let Some(certificate) = data_certificate() else {
        return plain_error_response(StatusCode::INTERNAL_SERVER_ERROR, "certificate unavailable");
    };
    ASSET_ROUTER.with_borrow(|asset_router| {
        match asset_router.serve_asset(&certificate, req) {
            Ok(response) => response,
            Err(_) => plain_error_response(StatusCode::INTERNAL_SERVER_ERROR, "failed to serve frontend asset"),
        }
    })
}

fn get_asset_headers(additional_headers: Vec<HeaderField>) -> Vec<HeaderField> {
    let mut headers = vec![
        (
            "strict-transport-security".to_string(),
            "max-age=31536000; includeSubDomains".to_string(),
        ),
        ("x-content-type-options".to_string(), "nosniff".to_string()),
        (
            "content-security-policy".to_string(),
            "default-src 'self';              connect-src 'self' https://icp0.io https://*.icp0.io;              base-uri 'self';              img-src 'self' data:;              style-src 'self' 'unsafe-inline';              style-src-attr 'unsafe-inline';              form-action 'self';              object-src 'none';              frame-ancestors 'self' https://jupiter-faucet.com https://www.jupiter-faucet.com;              upgrade-insecure-requests"
                .to_string(),
        ),
        ("referrer-policy".to_string(), "no-referrer".to_string()),
        (
            "permissions-policy".to_string(),
            "accelerometer=(),autoplay=(),camera=(),display-capture=(),geolocation=(),gyroscope=(),magnetometer=(),microphone=(),midi=(),payment=(),picture-in-picture=(),publickey-credentials-get=(),screen-wake-lock=(),usb=(),web-share=(),xr-spatial-tracking=()"
                .to_string(),
        ),
        (
            "cross-origin-embedder-policy".to_string(),
            "require-corp".to_string(),
        ),
        (
            "cross-origin-opener-policy".to_string(),
            "same-origin".to_string(),
        ),
        (
            "cross-origin-resource-policy".to_string(),
            "same-origin".to_string(),
        ),
    ];
    headers.extend(additional_headers);
    headers
}


#[cfg(test)]
mod tests {
    use super::*;

    fn header_value<'a>(response: &'a HttpResponse<'static>, name: &str) -> Option<&'a str> {
        response
            .headers()
            .iter()
            .find(|(header, _)| header.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn request_target_routes_metrics_separately() {
        assert_eq!(request_target_for_path("/metrics"), RequestTarget::Metrics);
        assert_eq!(request_target_for_path("/"), RequestTarget::Asset);
        assert_eq!(request_target_for_path("/assets/app.js"), RequestTarget::Asset);
        assert_eq!(request_target_for_path("/does-not-exist"), RequestTarget::Asset);
    }

    #[test]
    fn http_request_returns_bad_request_for_malformed_urls() {
        let request = HttpRequest::get("http://[").build();
        let response = http_request(request);
        assert_eq!(response.status_code(), StatusCode::BAD_REQUEST);
        assert_eq!(response.body(), b"bad request path");
        assert_eq!(header_value(&response, "cache-control"), Some(NO_CACHE_ASSET_CACHE_CONTROL));
    }

    #[test]
    fn plain_error_response_includes_security_and_cache_headers() {
        let response = plain_error_response(StatusCode::BAD_REQUEST, "bad request path");
        assert_eq!(response.status_code(), StatusCode::BAD_REQUEST);
        assert_eq!(response.body(), b"bad request path");
        assert_eq!(header_value(&response, "content-type"), Some("text/plain; charset=utf-8"));
        assert_eq!(header_value(&response, "cache-control"), Some(NO_CACHE_ASSET_CACHE_CONTROL));
        assert_eq!(header_value(&response, "x-content-type-options"), Some("nosniff"));
        assert!(header_value(&response, "content-security-policy").is_some());
    }

    #[test]
    fn asset_headers_include_expected_defaults_and_overrides() {
        let headers = get_asset_headers(vec![("cache-control".to_string(), IMMUTABLE_ASSET_CACHE_CONTROL.to_string())]);
        assert!(headers.iter().any(|(name, value)| name == "strict-transport-security" && value == "max-age=31536000; includeSubDomains"));
        assert!(headers.iter().any(|(name, value)| name == "cache-control" && value == IMMUTABLE_ASSET_CACHE_CONTROL));
        assert!(headers.iter().any(|(name, value)| name == "cross-origin-opener-policy" && value == "same-origin"));
    }

    #[test]
    fn metrics_cel_expression_is_response_only_not_skip_certification() {
        let cel_expr = metrics_cel_expression();
        assert_ne!(cel_expr, DefaultCelBuilder::skip_certification().to_string());
    }

    #[test]
    fn initialize_runtime_state_populates_certified_metrics_snapshot() {
        CERTIFIED_METRICS_SNAPSHOT.with(|stored| *stored.borrow_mut() = None);
        initialize_runtime_state();
        CERTIFIED_METRICS_SNAPSHOT.with(|stored| {
            let snapshot = stored.borrow();
            let snapshot = snapshot.as_ref().expect("expected certified metrics snapshot");
            assert!(!snapshot.body.is_empty());
            assert_eq!(
                snapshot
                    .headers
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
                    .map(|(_, value)| value.as_str()),
                Some("application/json")
            );
        });
    }

    #[test]
    fn build_certified_metrics_snapshot_uses_certified_metrics_headers() {
        certify_all_assets();
        ASSET_ROUTER.with_borrow(|asset_router| {
            let snapshot = build_certified_metrics_snapshot(asset_router);
            assert_eq!(
                snapshot
                    .headers
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
                    .map(|(_, value)| value.as_str()),
                Some("application/json")
            );
            assert_eq!(
                snapshot
                    .headers
                    .iter()
                    .find(|(name, _)| name == CERTIFICATE_EXPRESSION_HEADER_NAME)
                    .map(|(_, value)| value.as_str()),
                Some(snapshot.cel_expr.as_str())
            );
        });
    }
}

