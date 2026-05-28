use ic_asset_certification::{
    Asset, AssetCertificationError, AssetConfig, AssetEncoding, AssetFallbackConfig, AssetMap,
    AssetRouter,
};
use ic_cdk::{api::data_certificate, init, post_upgrade, query};
use ic_http_certification::{
    utils::add_v2_certificate_header, DefaultCelBuilder, DefaultResponseCertification,
    DefaultResponseOnlyCelExpression, HeaderField, HttpCertification, HttpCertificationPath,
    HttpCertificationTree, HttpCertificationTreeEntry, HttpRequest, HttpResponse, Method,
    StatusCode, CERTIFICATE_EXPRESSION_HEADER_NAME,
};
use include_dir::{include_dir, Dir};
use serde::Serialize;
#[cfg(not(test))]
use std::time::Duration;
use std::{cell::RefCell, collections::HashMap, rc::Rc};

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

#[cfg(not(test))]
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
    static HEAD_ASSETS: RefCell<HashMap<HeadAssetKey, CertifiedHeadAsset>> = RefCell::new(HashMap::new());
    static CERTIFIED_METRICS_SNAPSHOT: RefCell<Option<CertifiedMetricsSnapshot>> = const { RefCell::new(None) };
}

static ASSETS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/assets");
const IMMUTABLE_ASSET_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";
const NO_CACHE_ASSET_CACHE_CONTROL: &str = "public, no-cache, no-store";
const SOCIAL_PREVIEW_IMAGE_PATHS: [&str; 1] = ["og/preview-20260520.jpg"];
const PRIVATE_BUILD_MANIFEST_PATH: &str = "generated/frontend-bundle.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestTarget {
    Metrics,
    Asset,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct HeadAssetKey {
    path: String,
    match_kind: HeadAssetMatchKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum HeadAssetMatchKind {
    Exact,
    Fallback,
}

#[derive(Debug, Clone)]
struct CertifiedHeadAsset {
    response: HttpResponse<'static>,
    tree_entry: HttpCertificationTreeEntry<'static>,
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
        .with_headers(get_asset_headers_with_corp(
            "same-origin",
            vec![
                (
                    "content-type".to_string(),
                    "text/plain; charset=utf-8".to_string(),
                ),
                (
                    "cache-control".to_string(),
                    NO_CACHE_ASSET_CACHE_CONTROL.to_string(),
                ),
            ],
        ))
        .build()
}

fn collect_assets<'content, 'path>(
    dir: &'content Dir<'path>,
    assets: &mut Vec<Asset<'content, 'path>>,
) {
    for file in dir.files() {
        let path = file.path().to_string_lossy();
        if path != PRIVATE_BUILD_MANIFEST_PATH {
            assets.push(Asset::new(path, file.contents()));
        }
    }

    for dir in dir.dirs() {
        collect_assets(dir, assets);
    }
}

fn metrics_tree_path() -> HttpCertificationPath<'static> {
    HttpCertificationPath::exact("/metrics")
}

#[cfg(not(test))]
fn update_certified_data(root_hash: &[u8]) {
    ic_cdk::api::certified_data_set(root_hash);
}

#[cfg(test)]
fn update_certified_data(root_hash: &[u8]) {
    let _ = root_hash;
}

#[cfg(not(test))]
fn current_cycle_balance() -> u128 {
    ic_cdk::api::canister_cycle_balance()
}

#[cfg(test)]
fn current_cycle_balance() -> u128 {
    0
}

fn metrics_cel_expression() -> DefaultResponseOnlyCelExpression<'static> {
    DefaultCelBuilder::response_only_certification()
        .with_response_certification(DefaultResponseCertification::response_header_exclusions(
            vec![],
        ))
        .build()
}

fn build_certified_metrics_snapshot(
    asset_router: &AssetRouter<'static>,
) -> CertifiedMetricsSnapshot {
    let metrics = Metrics {
        num_assets: asset_router.get_assets().len(),
        num_fallback_assets: asset_router.get_fallback_assets().len(),
        cycle_balance: current_cycle_balance(),
    };
    let body = serde_json::to_vec(&metrics).expect("failed to serialize metrics");
    let cel_expr = metrics_cel_expression();
    let headers = get_asset_headers_with_corp(
        "same-origin",
        vec![
            (
                CERTIFICATE_EXPRESSION_HEADER_NAME.to_string(),
                cel_expr.to_string(),
            ),
            ("content-type".to_string(), "application/json".to_string()),
            (
                "cache-control".to_string(),
                NO_CACHE_ASSET_CACHE_CONTROL.to_string(),
            ),
        ],
    );
    CertifiedMetricsSnapshot {
        body,
        headers,
        cel_expr,
    }
}

#[cfg(not(test))]
fn install_metrics_refresh_timer() {
    ic_cdk_timers::set_timer_interval(
        Duration::from_secs(METRICS_REFRESH_INTERVAL_SECS),
        || async {
            refresh_certified_metrics_snapshot();
        },
    );
}

#[cfg(test)]
fn install_metrics_refresh_timer() {}

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
        let metrics_tree_entry =
            HttpCertificationTreeEntry::new(&metrics_tree_path, certification);

        HTTP_TREE.with(|tree| {
            let mut tree = tree.borrow_mut();
            tree.delete_by_path(&metrics_tree_path);
            tree.insert(&metrics_tree_entry);
            update_certified_data(&tree.root_hash());
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
            headers: get_asset_headers_with_corp(
                "same-origin",
                vec![(
                    "cache-control".to_string(),
                    NO_CACHE_ASSET_CACHE_CONTROL.to_string(),
                )],
            ),
            fallback_for: vec![],
            aliased_by: vec!["/".to_string()],
            encodings: compressed_encodings.clone(),
        },
        AssetConfig::File {
            path: "404.html".to_string(),
            content_type: Some("text/html".to_string()),
            headers: get_asset_headers_with_corp(
                "same-origin",
                vec![(
                    "cache-control".to_string(),
                    NO_CACHE_ASSET_CACHE_CONTROL.to_string(),
                )],
            ),
            fallback_for: vec![AssetFallbackConfig {
                scope: "/".to_string(),
                status_code: Some(StatusCode::NOT_FOUND),
            }],
            aliased_by: vec![
                "/404".to_string(),
                "/404/".to_string(),
                "/404.html".to_string(),
            ],
            encodings: compressed_encodings.clone(),
        },
        AssetConfig::Pattern {
            pattern: "**/*.js".to_string(),
            content_type: Some("text/javascript".to_string()),
            headers: get_asset_headers_with_corp(
                "same-origin",
                vec![(
                    "cache-control".to_string(),
                    IMMUTABLE_ASSET_CACHE_CONTROL.to_string(),
                )],
            ),
            encodings: compressed_encodings.clone(),
        },
        AssetConfig::Pattern {
            pattern: "**/*.css".to_string(),
            content_type: Some("text/css".to_string()),
            headers: get_asset_headers_with_corp(
                "same-origin",
                vec![(
                    "cache-control".to_string(),
                    IMMUTABLE_ASSET_CACHE_CONTROL.to_string(),
                )],
            ),
            encodings: compressed_encodings,
        },
        AssetConfig::File {
            path: "og/preview-20260520.jpg".to_string(),
            content_type: Some("image/jpeg".to_string()),
            headers: get_asset_headers_for_path(
                "og/preview-20260520.jpg",
                vec![(
                    "cache-control".to_string(),
                    IMMUTABLE_ASSET_CACHE_CONTROL.to_string(),
                )],
            ),
            fallback_for: vec![],
            aliased_by: vec![],
            encodings: vec![],
        },
        AssetConfig::Pattern {
            pattern: "**/*.ico".to_string(),
            content_type: Some("image/x-icon".to_string()),
            headers: get_asset_headers_with_corp(
                "same-origin",
                vec![(
                    "cache-control".to_string(),
                    IMMUTABLE_ASSET_CACHE_CONTROL.to_string(),
                )],
            ),
            encodings: vec![],
        },
        AssetConfig::Pattern {
            pattern: "**/*.png".to_string(),
            content_type: Some("image/png".to_string()),
            headers: get_asset_headers_with_corp(
                "same-origin",
                vec![(
                    "cache-control".to_string(),
                    IMMUTABLE_ASSET_CACHE_CONTROL.to_string(),
                )],
            ),
            encodings: vec![],
        },
        AssetConfig::Pattern {
            pattern: "**/*.{jpg,jpeg}".to_string(),
            content_type: Some("image/jpeg".to_string()),
            headers: get_asset_headers_with_corp(
                "same-origin",
                vec![(
                    "cache-control".to_string(),
                    IMMUTABLE_ASSET_CACHE_CONTROL.to_string(),
                )],
            ),
            encodings: vec![],
        },
        AssetConfig::Pattern {
            pattern: "**/*.webp".to_string(),
            content_type: Some("image/webp".to_string()),
            headers: get_asset_headers_with_corp(
                "same-origin",
                vec![(
                    "cache-control".to_string(),
                    IMMUTABLE_ASSET_CACHE_CONTROL.to_string(),
                )],
            ),
            encodings: vec![],
        },
        AssetConfig::Pattern {
            pattern: "**/*.svg".to_string(),
            content_type: Some("image/svg+xml".to_string()),
            headers: get_asset_headers_with_corp(
                "same-origin",
                vec![(
                    "cache-control".to_string(),
                    IMMUTABLE_ASSET_CACHE_CONTROL.to_string(),
                )],
            ),
            encodings: vec![],
        },
        AssetConfig::File {
            path: ".well-known/ic-domains".to_string(),
            content_type: Some("text/plain".to_string()),
            headers: get_asset_headers_with_corp(
                "same-origin",
                vec![(
                    "cache-control".to_string(),
                    NO_CACHE_ASSET_CACHE_CONTROL.to_string(),
                )],
            ),
            fallback_for: vec![],
            aliased_by: vec![],
            encodings: vec![],
        },
    ];

    let mut assets = Vec::new();
    collect_assets(&ASSETS_DIR, &mut assets);

    ASSET_ROUTER.with_borrow_mut(|asset_router| {
        if let Err(err) = asset_router.certify_assets(assets, asset_configs) {
            ic_cdk::trap(format!("failed to certify frontend assets: {err}"));
        }

        update_certified_data(&asset_router.root_hash());
    });

    if let Err(err) = certify_head_assets(&ASSETS_DIR) {
        ic_cdk::trap(format!("failed to certify frontend HEAD assets: {err}"));
    }

    HTTP_TREE.with(|tree| update_certified_data(&tree.borrow().root_hash()));
}

fn certify_head_assets(dir: &Dir<'static>) -> Result<(), String> {
    let mut head_assets = HashMap::new();

    let index_file = dir
        .get_file("index.html")
        .ok_or_else(|| "index.html is missing from frontend assets".to_string())?;
    let index_headers = asset_headers_for_path("index.html", index_file.contents().len());
    insert_head_asset(
        &mut head_assets,
        "/",
        StatusCode::OK,
        index_headers.clone(),
        HeadAssetMatchKind::Exact,
    )?;
    insert_head_asset(
        &mut head_assets,
        "/index.html",
        StatusCode::OK,
        index_headers,
        HeadAssetMatchKind::Exact,
    )?;

    for path in SOCIAL_PREVIEW_IMAGE_PATHS {
        let preview_file = dir
            .get_file(path)
            .ok_or_else(|| format!("{path} is missing from frontend assets"))?;
        insert_head_asset(
            &mut head_assets,
            &format!("/{path}"),
            StatusCode::OK,
            asset_headers_for_path(path, preview_file.contents().len()),
            HeadAssetMatchKind::Exact,
        )?;
    }

    let not_found_file = dir
        .get_file("404.html")
        .ok_or_else(|| "404.html is missing from frontend assets".to_string())?;
    let not_found_headers = asset_headers_for_path("404.html", not_found_file.contents().len());
    insert_head_asset(
        &mut head_assets,
        "/",
        StatusCode::NOT_FOUND,
        not_found_headers,
        HeadAssetMatchKind::Fallback,
    )?;

    HEAD_ASSETS.with(|stored| *stored.borrow_mut() = head_assets);
    Ok(())
}

fn insert_head_asset(
    head_assets: &mut HashMap<HeadAssetKey, CertifiedHeadAsset>,
    path: &str,
    status_code: StatusCode,
    headers: Vec<HeaderField>,
    match_kind: HeadAssetMatchKind,
) -> Result<(), String> {
    let response = HttpResponse::builder()
        .with_status_code(status_code)
        .with_body(Vec::new())
        .with_headers(headers)
        .build();
    let request = HttpRequest::builder()
        .with_method(Method::HEAD)
        .with_url(path)
        .build();
    let cel_expr = DefaultCelBuilder::full_certification()
        .with_response_certification(DefaultResponseCertification::response_header_exclusions(
            vec![],
        ))
        .build();
    let certification = HttpCertification::full(&cel_expr, &request, &response, None)
        .map_err(|err| err.to_string())?;
    let certification_path = match match_kind {
        HeadAssetMatchKind::Exact => HttpCertificationPath::exact(path.to_string()),
        HeadAssetMatchKind::Fallback => HttpCertificationPath::wildcard(path.to_string()),
    };
    let tree_entry = HttpCertificationTreeEntry::new(certification_path, certification);

    HTTP_TREE.with(|tree| tree.borrow_mut().insert(&tree_entry));
    head_assets.insert(
        HeadAssetKey {
            path: path.to_string(),
            match_kind,
        },
        CertifiedHeadAsset {
            response,
            tree_entry,
        },
    );

    Ok(())
}

fn asset_headers_for_path(path: &str, content_length: usize) -> Vec<HeaderField> {
    let cache_control =
        if path == "index.html" || path == "404.html" || path == ".well-known/ic-domains" {
            NO_CACHE_ASSET_CACHE_CONTROL
        } else {
            IMMUTABLE_ASSET_CACHE_CONTROL
        };

    let mut headers = vec![("content-length".to_string(), content_length.to_string())];
    headers.extend(get_asset_headers_for_path(
        path,
        vec![("cache-control".to_string(), cache_control.to_string())],
    ));

    if let Some(content_type) = content_type_for_path(path) {
        headers.push(("content-type".to_string(), content_type.to_string()));
    }

    headers.push((
        CERTIFICATE_EXPRESSION_HEADER_NAME.to_string(),
        DefaultCelBuilder::full_certification()
            .with_response_certification(DefaultResponseCertification::response_header_exclusions(
                vec![],
            ))
            .build()
            .to_string(),
    ));

    headers
}

fn content_type_for_path(path: &str) -> Option<&'static str> {
    match path {
        "index.html" | "404.html" => Some("text/html"),
        ".well-known/ic-domains" => Some("text/plain"),
        _ if path.ends_with(".js") => Some("text/javascript"),
        _ if path.ends_with(".css") => Some("text/css"),
        _ if path.ends_with(".ico") => Some("image/x-icon"),
        _ if path.ends_with(".png") => Some("image/png"),
        _ if path.ends_with(".jpg") || path.ends_with(".jpeg") => Some("image/jpeg"),
        _ if path.ends_with(".webp") => Some("image/webp"),
        _ if path.ends_with(".svg") => Some("image/svg+xml"),
        _ => None,
    }
}

fn serve_metrics() -> HttpResponse<'static> {
    let Some(snapshot) = CERTIFIED_METRICS_SNAPSHOT.with(|stored| stored.borrow().clone()) else {
        return plain_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "metrics snapshot unavailable",
        );
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
        let certification =
            match HttpCertification::response_only(&snapshot.cel_expr, &response, None) {
                Ok(certification) => certification,
                Err(_) => {
                    return plain_error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "metrics certification unavailable",
                    )
                }
            };
        let metrics_tree_entry =
            HttpCertificationTreeEntry::new(&metrics_tree_path, certification);
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
    serve_asset_with_certificate(&certificate, req)
}

fn serve_asset_with_certificate(certificate: &[u8], req: &HttpRequest) -> HttpResponse<'static> {
    if req.method() == Method::HEAD {
        return serve_head_asset(certificate, req);
    }

    ASSET_ROUTER.with_borrow(
        |asset_router| match asset_router.serve_asset(certificate, req) {
            Ok(response) => response,
            Err(err) => asset_error_response(&err),
        },
    )
}

fn serve_head_asset(certificate: &[u8], req: &HttpRequest) -> HttpResponse<'static> {
    let path = match req.get_path() {
        Ok(path) => path,
        Err(err) => return asset_error_response(&AssetCertificationError::from(err)),
    };

    HEAD_ASSETS.with_borrow(
        |head_assets| match find_head_asset(head_assets, &path).cloned() {
            Some(mut asset) => {
                let witness = HTTP_TREE.with(|tree| {
                    tree.borrow()
                        .witness(&asset.tree_entry, &path)
                        .map_err(AssetCertificationError::from)
                });
                match witness {
                    Ok(witness) => {
                        add_v2_certificate_header(
                            certificate,
                            &mut asset.response,
                            &witness,
                            &asset.tree_entry.path.to_expr_path(),
                        );
                        asset.response
                    }
                    Err(err) => asset_error_response(&err),
                }
            }
            None => asset_error_response(&AssetCertificationError::NoAssetMatchingRequestUrl {
                request_url: path,
            }),
        },
    )
}

fn find_head_asset<'a>(
    head_assets: &'a HashMap<HeadAssetKey, CertifiedHeadAsset>,
    path: &str,
) -> Option<&'a CertifiedHeadAsset> {
    if let Some(asset) = head_assets.get(&HeadAssetKey {
        path: path.to_string(),
        match_kind: HeadAssetMatchKind::Exact,
    }) {
        return Some(asset);
    }

    let mut url_scopes = path.split('/').collect::<Vec<_>>();
    url_scopes.pop();

    while !url_scopes.is_empty() {
        let mut scope = url_scopes.join("/");
        scope.push('/');

        for candidate_scope in [scope.as_str(), scope.trim_end_matches('/')] {
            if let Some(asset) = head_assets.get(&HeadAssetKey {
                path: candidate_scope.to_string(),
                match_kind: HeadAssetMatchKind::Fallback,
            }) {
                return Some(asset);
            }
        }

        url_scopes.pop();
    }

    None
}

fn asset_error_response(err: &AssetCertificationError) -> HttpResponse<'static> {
    match err {
        AssetCertificationError::NoAssetMatchingRequestUrl { .. } => {
            plain_error_response(StatusCode::NOT_FOUND, "not found")
        }
        _ => plain_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to serve frontend asset",
        ),
    }
}

fn get_asset_headers_for_path(
    path: &str,
    additional_headers: Vec<HeaderField>,
) -> Vec<HeaderField> {
    let corp = if is_social_preview_image(path) {
        "cross-origin"
    } else {
        "same-origin"
    };
    get_asset_headers_with_corp(corp, additional_headers)
}

fn is_social_preview_image(path: &str) -> bool {
    SOCIAL_PREVIEW_IMAGE_PATHS.contains(&path)
}

fn get_asset_headers_with_corp(
    corp: &str,
    additional_headers: Vec<HeaderField>,
) -> Vec<HeaderField> {
    let mut headers = vec![
        (
            "strict-transport-security".to_string(),
            "max-age=31536000; includeSubDomains".to_string(),
        ),
        ("x-content-type-options".to_string(), "nosniff".to_string()),
        (
            "content-security-policy".to_string(),
            "default-src 'self';              connect-src 'self' https://icp0.io https://*.icp0.io;              base-uri 'self';              script-src 'self';              img-src 'self' data:;              style-src 'self' 'unsafe-inline';              style-src-attr 'unsafe-inline';              worker-src 'none';              child-src 'none';              frame-src 'none';              manifest-src 'self';              form-action 'self';              object-src 'none';              frame-ancestors 'self' https://jupiter-faucet.com https://www.jupiter-faucet.com;              upgrade-insecure-requests"
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
            corp.to_string(),
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

    fn generated_app_js_path() -> String {
        let generated_dir = ASSETS_DIR
            .get_dir("generated")
            .expect("generated assets directory should be embedded");
        generated_dir
            .files()
            .find_map(|file| {
                let path = file.path().to_string_lossy();
                (path.starts_with("generated/app.") && path.ends_with(".js"))
                    .then(|| format!("/{path}"))
            })
            .expect("generated app bundle should be embedded")
    }

    #[test]
    fn request_target_routes_metrics_separately() {
        assert_eq!(request_target_for_path("/metrics"), RequestTarget::Metrics);
        assert_eq!(request_target_for_path("/"), RequestTarget::Asset);
        assert_eq!(
            request_target_for_path("/assets/app.js"),
            RequestTarget::Asset
        );
        assert_eq!(
            request_target_for_path("/does-not-exist"),
            RequestTarget::Asset
        );
    }

    #[test]
    fn http_request_returns_bad_request_for_malformed_urls() {
        let request = HttpRequest::get("http://[").build();
        let response = http_request(request);
        assert_eq!(response.status_code(), StatusCode::BAD_REQUEST);
        assert_eq!(response.body(), b"bad request path");
        assert_eq!(
            header_value(&response, "cache-control"),
            Some(NO_CACHE_ASSET_CACHE_CONTROL)
        );
    }

    #[test]
    fn plain_error_response_includes_security_and_cache_headers() {
        let response = plain_error_response(StatusCode::BAD_REQUEST, "bad request path");
        assert_eq!(response.status_code(), StatusCode::BAD_REQUEST);
        assert_eq!(response.body(), b"bad request path");
        assert_eq!(
            header_value(&response, "content-type"),
            Some("text/plain; charset=utf-8")
        );
        assert_eq!(
            header_value(&response, "cache-control"),
            Some(NO_CACHE_ASSET_CACHE_CONTROL)
        );
        assert_eq!(
            header_value(&response, "x-content-type-options"),
            Some("nosniff")
        );
        assert!(header_value(&response, "content-security-policy").is_some());
    }

    #[test]
    fn no_asset_matching_request_url_maps_to_not_found() {
        let err = AssetCertificationError::NoAssetMatchingRequestUrl {
            request_url: "/missing.css".to_string(),
        };
        let response = asset_error_response(&err);

        assert_eq!(response.status_code(), StatusCode::NOT_FOUND);
        assert_eq!(response.body(), b"not found");
        assert_eq!(
            header_value(&response, "content-type"),
            Some("text/plain; charset=utf-8")
        );
        assert_eq!(
            header_value(&response, "cache-control"),
            Some(NO_CACHE_ASSET_CACHE_CONTROL)
        );
    }

    #[test]
    fn generated_frontend_bundle_manifest_is_not_routable() {
        certify_all_assets();

        let request = HttpRequest::get(format!("/{PRIVATE_BUILD_MANIFEST_PATH}")).build();
        let response = serve_asset_with_certificate(b"test-certificate", &request);

        assert_eq!(response.status_code(), StatusCode::NOT_FOUND);
        assert_ne!(
            header_value(&response, "content-type"),
            Some("application/json")
        );
    }

    #[test]
    fn generated_frontend_bundle_manifest_head_is_not_routable() {
        certify_all_assets();

        let request = HttpRequest::builder()
            .with_method(Method::HEAD)
            .with_url(format!("/{PRIVATE_BUILD_MANIFEST_PATH}"))
            .build();
        let response = serve_asset_with_certificate(b"test-certificate", &request);

        assert_eq!(response.status_code(), StatusCode::NOT_FOUND);
        assert_eq!(response.body(), b"");
        assert_ne!(
            header_value(&response, "content-type"),
            Some("application/json")
        );
    }

    #[test]
    fn generated_app_bundle_remains_routable_as_javascript() {
        certify_all_assets();

        let request = HttpRequest::get(generated_app_js_path()).build();
        let response = serve_asset_with_certificate(b"test-certificate", &request);

        assert_eq!(response.status_code(), StatusCode::OK);
        assert_eq!(
            header_value(&response, "content-type"),
            Some("text/javascript")
        );
        assert!(!response.body().is_empty());
    }

    #[test]
    fn certified_root_fallback_is_not_found_page_not_index() {
        certify_all_assets();

        ASSET_ROUTER.with_borrow(|asset_router| {
            let root_index = asset_router
                .get_assets()
                .get("/", None, None)
                .expect("expected root alias to serve index.html");
            let root_fallback = asset_router
                .get_fallback_assets()
                .get("/", None, None)
                .expect("expected root fallback to serve 404.html");

            assert_eq!(root_index.status_code(), StatusCode::OK);
            assert_eq!(root_fallback.status_code(), StatusCode::NOT_FOUND);
            assert!(String::from_utf8_lossy(root_fallback.body()).contains("Not Found"));
        });
    }

    #[test]
    fn head_root_serves_index_metadata() {
        certify_all_assets();

        let get_request = HttpRequest::get("/").build();
        let get_response = serve_asset_with_certificate(b"test-certificate", &get_request);
        let head_request = HttpRequest::builder()
            .with_method(Method::HEAD)
            .with_url("/")
            .build();
        let head_response = serve_asset_with_certificate(b"test-certificate", &head_request);

        assert_eq!(head_response.status_code(), StatusCode::OK);
        assert_eq!(header_value(&head_response, "content-type"), Some("text/html"));
        assert_eq!(
            header_value(&head_response, "cache-control"),
            header_value(&get_response, "cache-control")
        );
        assert_eq!(head_response.body(), b"");
        assert!(header_value(&head_response, "ic-certificate").is_some());
    }

    #[test]
    fn head_index_html_serves_index_metadata() {
        certify_all_assets();

        let request = HttpRequest::builder()
            .with_method(Method::HEAD)
            .with_url("/index.html")
            .build();
        let response = serve_asset_with_certificate(b"test-certificate", &request);

        assert_eq!(response.status_code(), StatusCode::OK);
        assert_eq!(header_value(&response, "content-type"), Some("text/html"));
        assert_eq!(
            header_value(&response, "cache-control"),
            Some(NO_CACHE_ASSET_CACHE_CONTROL)
        );
        assert_eq!(response.body(), b"");
        assert!(header_value(&response, "ic-certificate").is_some());
    }

    #[test]
    fn get_preview_jpg_returns_404_after_legacy_asset_removal() {
        certify_all_assets();

        let request = HttpRequest::get("/preview.jpg").build();
        let response = serve_asset_with_certificate(b"test-certificate", &request);

        assert_eq!(response.status_code(), StatusCode::NOT_FOUND);
        assert_ne!(header_value(&response, "content-type"), Some("image/jpeg"));
    }

    #[test]
    fn get_cache_busted_preview_jpg_serves_cross_origin_image_asset() {
        certify_all_assets();

        let request = HttpRequest::get("/og/preview-20260520.jpg").build();
        let response = serve_asset_with_certificate(b"test-certificate", &request);

        assert_eq!(response.status_code(), StatusCode::OK);
        assert_eq!(header_value(&response, "content-type"), Some("image/jpeg"));
        assert_eq!(
            header_value(&response, "cross-origin-resource-policy"),
            Some("cross-origin")
        );
        assert!(!response.body().is_empty());
    }

    #[test]
    fn head_preview_jpg_returns_404_after_legacy_head_entry_removal() {
        certify_all_assets();

        let request = HttpRequest::builder()
            .with_method(Method::HEAD)
            .with_url("/preview.jpg")
            .build();
        let response = serve_asset_with_certificate(b"test-certificate", &request);

        assert_eq!(response.status_code(), StatusCode::NOT_FOUND);
        assert_ne!(header_value(&response, "content-type"), Some("image/jpeg"));
        assert_eq!(response.body(), b"");
        assert!(header_value(&response, "ic-certificate").is_some());
    }

    #[test]
    fn head_cache_busted_preview_jpg_serves_certified_cross_origin_image_headers_without_body() {
        certify_all_assets();

        let request = HttpRequest::builder()
            .with_method(Method::HEAD)
            .with_url("/og/preview-20260520.jpg")
            .build();
        let response = serve_asset_with_certificate(b"test-certificate", &request);

        assert_eq!(response.status_code(), StatusCode::OK);
        assert_eq!(header_value(&response, "content-type"), Some("image/jpeg"));
        assert_eq!(
            header_value(&response, "cache-control"),
            Some(IMMUTABLE_ASSET_CACHE_CONTROL)
        );
        assert_eq!(response.body(), b"");
        assert!(header_value(&response, "ic-certificate").is_some());
        assert_eq!(
            header_value(&response, "cross-origin-resource-policy"),
            Some("cross-origin")
        );
    }

    #[test]
    fn index_html_uses_cache_busted_social_preview_image_url() {
        let index_html = ASSETS_DIR
            .get_file("index.html")
            .expect("index.html should be embedded in frontend assets");
        let index_html = std::str::from_utf8(index_html.contents())
            .expect("index.html should be valid utf-8");
        let preview_url = "https://jupiter-faucet.com/og/preview-20260520.jpg";

        assert!(index_html.contains(&format!(
            r#"<meta property="og:image" content="{preview_url}" />"#
        )));
        assert!(index_html.contains(&format!(
            r#"<meta property="og:image:secure_url" content="{preview_url}" />"#
        )));
        assert!(index_html.contains(&format!(
            r#"<meta name="twitter:image" content="{preview_url}" />"#
        )));
        assert!(!index_html.contains("/preview.jpg"));
    }

    #[test]
    fn head_compressible_asset_is_not_served_as_unencoded_asset() {
        certify_all_assets();

        let request = HttpRequest::builder()
            .with_method(Method::HEAD)
            .with_url("/base.css")
            .build();
        let response = serve_asset_with_certificate(b"test-certificate", &request);

        assert_eq!(response.status_code(), StatusCode::NOT_FOUND);
        assert_ne!(header_value(&response, "content-type"), Some("text/css"));
    }

    #[test]
    fn head_unknown_path_returns_404_not_index() {
        certify_all_assets();

        let request = HttpRequest::builder()
            .with_method(Method::HEAD)
            .with_url("/missing-path")
            .build();
        let response = serve_asset_with_certificate(b"test-certificate", &request);

        assert_eq!(response.status_code(), StatusCode::NOT_FOUND);
        assert_eq!(header_value(&response, "content-type"), Some("text/html"));
        assert_eq!(
            header_value(&response, "cache-control"),
            Some(NO_CACHE_ASSET_CACHE_CONTROL)
        );
        assert_eq!(response.body(), b"");
        assert!(header_value(&response, "ic-certificate").is_some());
    }

    #[test]
    fn asset_headers_include_expected_defaults_and_overrides() {
        let headers = get_asset_headers_with_corp(
            "same-origin",
            vec![(
                "cache-control".to_string(),
                IMMUTABLE_ASSET_CACHE_CONTROL.to_string(),
            )],
        );
        assert!(headers
            .iter()
            .any(|(name, value)| name == "strict-transport-security"
                && value == "max-age=31536000; includeSubDomains"));
        assert!(
            headers
                .iter()
                .any(|(name, value)| name == "cache-control"
                    && value == IMMUTABLE_ASSET_CACHE_CONTROL)
        );
        assert!(headers
            .iter()
            .any(|(name, value)| name == "cross-origin-opener-policy" && value == "same-origin"));
    }

    #[test]
    fn csp_spells_out_explicit_frontend_directives() {
        let headers = get_asset_headers_with_corp("same-origin", vec![]);
        let csp = headers
            .iter()
            .find(|(name, _)| name == "content-security-policy")
            .map(|(_, value)| value.as_str())
            .expect("security headers should include CSP");

        for directive in [
            "default-src 'self'",
            "script-src 'self'",
            "worker-src 'none'",
            "child-src 'none'",
            "frame-src 'none'",
            "manifest-src 'self'",
            "style-src 'self' 'unsafe-inline'",
            "style-src-attr 'unsafe-inline'",
            "frame-ancestors 'self' https://jupiter-faucet.com https://www.jupiter-faucet.com",
        ] {
            assert!(
                csp.contains(directive),
                "expected CSP to contain {directive:?}, got {csp:?}"
            );
        }
    }

    #[test]
    fn metrics_cel_expression_is_response_only_not_skip_certification() {
        let cel_expr = metrics_cel_expression();
        assert_ne!(
            cel_expr.to_string(),
            DefaultCelBuilder::skip_certification().to_string()
        );
    }

    #[test]
    fn initialize_runtime_state_populates_certified_metrics_snapshot() {
        CERTIFIED_METRICS_SNAPSHOT.with(|stored| *stored.borrow_mut() = None);
        initialize_runtime_state();
        CERTIFIED_METRICS_SNAPSHOT.with(|stored| {
            let snapshot = stored.borrow();
            let snapshot = snapshot
                .as_ref()
                .expect("expected certified metrics snapshot");
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
            let expected_cel_expr = snapshot.cel_expr.to_string();
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
                Some(expected_cel_expr.as_str())
            );
        });
    }
}
