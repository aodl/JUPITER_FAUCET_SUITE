use ic_asset_certification::{
    Asset, AssetConfig, AssetEncoding, AssetFallbackConfig, AssetMap, AssetRouter,
};
use ic_cdk::{
    api::{canister_cycle_balance, certified_data_set, data_certificate},
    init, post_upgrade, query,
};
use ic_http_certification::{
    utils::add_v2_certificate_header, DefaultCelBuilder, HeaderField, HttpCertification,
    HttpCertificationPath, HttpCertificationTree, HttpCertificationTreeEntry, HttpRequest,
    HttpResponse, StatusCode, CERTIFICATE_EXPRESSION_HEADER_NAME,
};
use include_dir::{include_dir, Dir};
use serde::Serialize;
use std::{cell::RefCell, rc::Rc};

#[derive(Debug, Clone, Serialize)]
pub struct Metrics {
    pub num_assets: usize,
    pub num_fallback_assets: usize,
    pub cycle_balance: u128,
}

#[init]
fn init() {
    certify_all_assets();
}

#[post_upgrade]
fn post_upgrade() {
    init();
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

    HTTP_TREE.with(|tree| {
        let mut tree = tree.borrow_mut();
        let metrics_tree_path = HttpCertificationPath::exact("/metrics");
        let metrics_certification = HttpCertification::skip();
        let metrics_tree_entry =
            HttpCertificationTreeEntry::new(metrics_tree_path, metrics_certification);
        tree.insert(&metrics_tree_entry);
    });

    ASSET_ROUTER.with_borrow_mut(|asset_router| {
        if let Err(err) = asset_router.certify_assets(assets, asset_configs) {
            ic_cdk::trap(&format!("failed to certify frontend assets: {err}"));
        }

        certified_data_set(&asset_router.root_hash());
    });
}

fn serve_metrics() -> HttpResponse<'static> {
    ASSET_ROUTER.with_borrow(|asset_router| {
        let metrics = Metrics {
            num_assets: asset_router.get_assets().len(),
            num_fallback_assets: asset_router.get_fallback_assets().len(),
            cycle_balance: canister_cycle_balance(),
        };
        let body = serde_json::to_vec(&metrics).expect("failed to serialize metrics");
        let headers = get_asset_headers(vec![
            (
                CERTIFICATE_EXPRESSION_HEADER_NAME.to_string(),
                DefaultCelBuilder::skip_certification().to_string(),
            ),
            ("content-type".to_string(), "application/json".to_string()),
            (
                "cache-control".to_string(),
                NO_CACHE_ASSET_CACHE_CONTROL.to_string(),
            ),
        ]);
        let mut response = HttpResponse::builder()
            .with_status_code(StatusCode::OK)
            .with_body(body)
            .with_headers(headers)
            .build();

        let Some(certificate) = data_certificate() else {
            return plain_error_response(StatusCode::INTERNAL_SERVER_ERROR, "certificate unavailable");
        };

        HTTP_TREE.with(|tree| {
            let tree = tree.borrow();
            let metrics_tree_path = HttpCertificationPath::exact("/metrics");
            let metrics_certification = HttpCertification::skip();
            let metrics_tree_entry =
                HttpCertificationTreeEntry::new(&metrics_tree_path, metrics_certification);
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
}
