use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
    future::Future,
    io::SeekFrom,
    pin::Pin,
    str::FromStr,
    sync::Arc,
    task::Poll,
    time::Duration,
};

use axum::{
    Json, RequestExt,
    body::{Body, HttpBody},
    extract::{OriginalUri, Query, Request, State},
    http::{Extensions, HeaderMap, HeaderName, HeaderValue, Method, Uri},
    middleware::Next,
    response::{IntoResponse, Response},
};

use axum_extra::extract::{CookieJar, cookie::Cookie};
use base64::{Engine, engine::general_purpose::STANDARD};
use bytes::{Bytes, BytesMut};
use chrono::Utc;
use futures::{Stream, stream::StreamExt as _};
use hmac::{Hmac, Mac};
use reqwest::{
    Body as ReqwestBody, Request as ReqwestRequest, RequestBuilder, Response as ReqwestResponse,
};
use rsa::{
    RsaPrivateKey, RsaPublicKey,
    pkcs1v15::{
        Signature as RsaSignature, SigningKey as RsaSigningKey, VerifyingKey as RsaVerifyingKey,
    },
    pkcs8::{DecodePrivateKey as _, DecodePublicKey as _, der::zeroize::Zeroizing},
    signature::{SignatureEncoding as _, SignerMut as _, Verifier as _},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use teloxide::types::UserId;
use tokio::{
    fs::File,
    io::{AsyncSeekExt as _, AsyncWriteExt},
};
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::{
    cache::Tokenable,
    context::{Context, RequestContext},
    db::{
        custom_type::{Id, TransportServiceType},
        entity::User,
    },
    error::{
        self, NotifyExchangeError, NotifyExchangeResult, NotifyExchangeResultExt, WwwAuthenticate,
    },
    model::cache::SessionCache,
    service::transport::telegram::{self, TelegramService},
};

// --- Constants for Headers ---
const HEADER_ADDITIONAL_HEADERS: &str = "x-ne-auth-additional-headers";
const HEADER_ALGO: &str = "x-ne-auth-algo";
const HEADER_BIZ_CODE: &str = "x-ne-auth-biz-code";
const HEADER_SHA256SUM: &str = "x-ne-auth-sha256sum";
const HEADER_SIGN: &str = "x-ne-auth-sign";
const HEADER_REQUEST_ID: &str = "x-ne-request-id";
const HEADER_RESPONSE_ID: &str = "x-ne-response-id";
/// The biz code expected in the response
const HEADER_EXPECTED_BIZ_CODE: &str = "x-ne-expected-auth-biz-code";

/// CSRF_TOKEN
pub const HEADER_CSRF_TOKEN: &str = "x-ne-csrf-token";

// `__Host-Http` is a cookie prefix for security
pub const COOKIE_SESSION_ID: &str = "__Host-Http-x-ne-session-id";

pub const SESSION_TIMEOUT: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignAlgo {
    Sha256Rsa,
}

impl Display for SignAlgo {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        let s = match self {
            SignAlgo::Sha256Rsa => "sha256-rsa",
        };
        f.write_str(s)
    }
}

impl FromStr for SignAlgo {
    type Err = NotifyExchangeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "sha256-rsa" => Ok(Self::Sha256Rsa),
            _ => Err(error::invalid_request(format!(
                "Unsupported algorithm: {}",
                s
            ))),
        }
    }
}

struct DataStream<B>(B);

impl Stream for DataStream<ReqwestBody> {
    type Item = Result<Bytes, <ReqwestBody as HttpBody>::Error>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context,
    ) -> Poll<Option<Self::Item>> {
        loop {
            return match futures::ready!(Pin::new(&mut self.0).poll_frame(cx)) {
                Some(Ok(frame)) => {
                    // skip non-data frames
                    if let Ok(buf) = frame.into_data() {
                        Poll::Ready(Some(Ok(buf)))
                    } else {
                        continue;
                    }
                }
                Some(Err(err)) => Poll::Ready(Some(Err(err))),
                None => Poll::Ready(None),
            };
        }
    }
}

#[derive(Clone)]
pub struct VerifiedHeaderMap(Arc<HeaderMap>);

pub trait HttpApiCalleeAuthHelper: Clone + 'static {
    fn context(&self) -> &Context;

    fn extend(
        &self,
        extensions: &mut Extensions,
        biz_code: &str,
        headers: &HeaderMap,
    ) -> impl Future<Output = NotifyExchangeResult<()>>;

    fn set_verified_public_key(
        &self,
        extensions: &Extensions,
        index: usize,
    ) -> impl Future<Output = NotifyExchangeResult<()>>;

    /// Return one or more public keys
    fn public_keys<'a>(
        &'a self,
        extensions: &'a Extensions,
        biz_code: &'a str,
        sign_algo: SignAlgo,
    ) -> impl Future<Output = NotifyExchangeResult<Vec<&'a Bytes>>>;

    fn response_biz_code(
        &self,
        biz_code: &str,
        sign_algo: SignAlgo,
    ) -> impl Future<Output = NotifyExchangeResult<String>>;

    /// pkcs8_der encoded private key
    fn private_key(
        &self,
        biz_code: &str,
        sign_algo: SignAlgo,
    ) -> impl Future<Output = NotifyExchangeResult<Zeroizing<Vec<u8>>>>;
}

pub trait HttpApiCallerAuthHelper: Sized {
    fn max_body_size_without_temp_file(&self) -> usize;

    fn temp_dir(&self) -> Option<&str>;

    fn call(
        &self,
        req: RequestBuilder,
    ) -> impl Future<Output = NotifyExchangeResult<ReqwestResponse>> {
        auth_api_call(self, req)
    }

    fn sign_algo(&self) -> impl Future<Output = NotifyExchangeResult<SignAlgo>>;

    fn request_biz_code(&self) -> impl Future<Output = NotifyExchangeResult<String>>;

    fn response_biz_code(&self) -> impl Future<Output = NotifyExchangeResult<Option<String>>>;

    fn private_key(&self) -> impl Future<Output = NotifyExchangeResult<Zeroizing<Vec<u8>>>>;

    /// Get the public key according to the biz code in the response.
    fn public_keys(
        &self,
        biz_code: &str,
    ) -> impl Future<Output = NotifyExchangeResult<Vec<&Bytes>>>;
}

#[repr(transparent)]
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
struct RequestIdCache(u8);

impl Tokenable for RequestIdCache {
    fn prefix() -> Cow<'static, str> {
        "auth_request_id".into()
    }
}

/// Parameters will be reused in the signing of response.
struct SignParams {
    biz_code: String,
    request_id: String,
    sign_algo: SignAlgo,
}

/// Sort then join key pairs, like: a=v1&b=v2
fn sort_then_join<S1, S2>(key_pairs: &mut [(S1, S2)]) -> String
where
    S1: AsRef<str>,
    S2: AsRef<str>,
{
    key_pairs.sort_by(|a, b| {
        if a.0.as_ref() == b.0.as_ref() {
            a.1.as_ref().cmp(b.1.as_ref())
        } else {
            a.0.as_ref().cmp(b.0.as_ref())
        }
    });
    key_pairs
        .iter_mut()
        .map(|(k, v)| format!("{}={}", k.as_ref(), v.as_ref()))
        .collect::<Vec<_>>()
        .join("&")
}

fn insert_header(
    headers: &mut HeaderMap,
    name: &'static str,
    value: &str,
) -> NotifyExchangeResult<()> {
    headers.insert(
        name,
        HeaderValue::from_str(value).map_err(|e| {
            tracing::error!("Header value contains invalid character: {e:?}");
            error::internal_server_error("Header value contains invalid character")
        })?,
    );
    Ok(())
}

fn get_header_with_default<'a>(
    headers: &'a HeaderMap,
    name: &str,
    default: &'a str,
) -> Result<&'a str, String> {
    let mut it = get_headers(headers, name);
    if let Some(v) = it.next() {
        if it.next().is_some() {
            Err(format!("Header appears more than once: {}", name))
        } else {
            v
        }
    } else {
        Ok(default)
    }
}

fn get_headers<'a>(
    headers: &'a HeaderMap,
    name: &str,
) -> impl Iterator<Item = Result<&'a str, String>> {
    headers.get_all(name).iter().map(move |v| {
        v.to_str()
            .map_err(|_| format!("Invalid header value for {}", name))
    })
}

fn get_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, String> {
    let mut it = get_headers(headers, name);
    if let Some(v) = it.next() {
        if it.next().is_some() {
            Err(format!("Header appears more than once: {}", name))
        } else {
            v
        }
    } else {
        Err(format!("Missing required header: {}", name))
    }
}

fn build_headers_sign_string<'a>(
    headers: &'a HeaderMap,
    basic_headers: &'a [&'a str],
) -> Result<(String, HeaderMap), String> {
    let additional_headers = get_header_with_default(headers, HEADER_ADDITIONAL_HEADERS, "")?;

    let mut header_key_pairs = vec![(HEADER_ADDITIONAL_HEADERS, additional_headers)];

    let mut included_headers = HashSet::new();
    included_headers.extend(basic_headers);
    included_headers.extend(additional_headers.split(',').map(|s| s.trim()));
    included_headers.remove(HEADER_ADDITIONAL_HEADERS);

    for header in included_headers {
        if header.is_empty() {
            continue;
        }
        let mut found = false;
        for value in get_headers(headers, header) {
            found = true;
            header_key_pairs.push((header, value?));
        }
        if !found {
            return Err(format!("Missing required header: {}", header));
        }
    }

    let mut sign_headers = HeaderMap::new();
    for (header, value) in &header_key_pairs {
        sign_headers.append(
            HeaderName::from_str(header).expect("Shouldn't be happened, name is used before"),
            HeaderValue::from_str(value)
                .expect("Shouldn't be happened, value is read from headers"),
        );
    }

    Ok((sort_then_join(&mut header_key_pairs), sign_headers))
}

fn build_response_sign_string(
    status: u16,
    headers: &HeaderMap,
) -> Result<(String, HeaderMap), String> {
    let (sorted_headers, sign_headers) = build_headers_sign_string(
        headers,
        &[
            HEADER_ALGO,
            HEADER_BIZ_CODE,
            HEADER_SHA256SUM,
            HEADER_REQUEST_ID,
            HEADER_RESPONSE_ID,
        ],
    )?;
    Ok((format!("{}\n{}", status, sorted_headers,), sign_headers))
}

fn build_request_sign_string(
    path_prefix: &str,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
) -> Result<(String, HeaderMap), String> {
    let path = uri.path();

    let Query(mut query_key_pairs): Query<Vec<(String, String)>> = Query::try_from_uri(uri)
        .map_err(|e| {
            tracing::debug!("Invalid query string: {e:?}");
            "Invalid request".to_string()
        })?;
    query_key_pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let sorted_query = sort_then_join(&mut query_key_pairs);

    let (sorted_headers, sign_headers) = build_headers_sign_string(
        headers,
        &[
            HEADER_ALGO,
            HEADER_BIZ_CODE,
            HEADER_SHA256SUM,
            HEADER_REQUEST_ID,
        ],
    )?;
    Ok((
        format!(
            "{}{}\n{}\n{}\n{}",
            path_prefix,
            path,
            sorted_query,
            method.to_string().to_uppercase(),
            sorted_headers
        ),
        sign_headers,
    ))
}

fn verify(
    sign_algo: SignAlgo,
    public_keys: &[&Bytes],
    sign_string: &str,
    signature: &str,
) -> NotifyExchangeResult<usize> {
    tracing::debug!(
        "To be verified string in hex: {}",
        hex::encode(sign_string.as_bytes())
    );
    match sign_algo {
        SignAlgo::Sha256Rsa => {
            let signature = STANDARD.decode(signature).map_err(|e| {
                tracing::debug!("Invalid base64 encoding: {e:?}");
                error::invalid_sign("Invalid base64 encoding")
            })?;
            let signature = RsaSignature::try_from(signature.as_slice()).map_err(|e| {
                tracing::debug!("Invalid signature: {e:?}");
                error::invalid_sign("Invalid signature")
            })?;

            for (index, public_key) in public_keys.iter().enumerate() {
                let Ok(public_key) = RsaPublicKey::from_public_key_der(public_key) else {
                    tracing::info!(
                        "Invalid public key format provided by helper, sign_algo: {sign_algo}"
                    );
                    continue;
                };
                let verifying_key = RsaVerifyingKey::<Sha256>::new(public_key);
                if let Err(e) = verifying_key.verify(sign_string.as_bytes(), &signature) {
                    tracing::debug!("Try the {index}th public key, error: {e:?}");
                    continue;
                }
                return Ok(index);
            }
            Err(error::invalid_sign("Invalid signature"))
        }
    }
}

fn get_header_for_callee_request<'a>(
    headers: &'a HeaderMap,
    name: &str,
) -> NotifyExchangeResult<&'a str> {
    get_header(headers, name).map_err(error::invalid_request)
}

fn get_header_for_caller_response<'a>(
    headers: &'a HeaderMap,
    name: &str,
) -> NotifyExchangeResult<&'a str> {
    get_header(headers, name).map_err(error::invalid_response)
}

async fn calc_body_sha256sum<S, E>(
    max_mem_size: usize,
    temp_dir: Option<&str>,
    temp_file_prefix: &str,
    mut stream: S,
) -> NotifyExchangeResult<(String, Option<File>, Bytes)>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
    E: Error + Send + Sync + 'static,
{
    let mut hasher = Sha256::new();
    let mut temp_file = None;

    let mut content_length = 0;
    let mut body_data = BytesMut::new();
    while let Some(data) = stream.next().await {
        let data = data.whatever("Failed to read stream")?;
        hasher.update(&data);
        content_length += data.len();
        if content_length > max_mem_size {
            let mut file = match temp_file {
                Some(temp_file) => temp_file,
                None => {
                    let (file, _) =
                        crate::temp_file_with_prefix(temp_dir, temp_file_prefix)?.into_parts();
                    let mut file = tokio::fs::File::from_std(file);
                    file.write_all(&body_data.freeze())
                        .await
                        .whatever("Unable write data to temp file")?;
                    body_data = BytesMut::new();
                    file
                }
            };
            file.write_all(&data)
                .await
                .whatever("Unable write data to temp file")?;
            temp_file = Some(file);
        } else {
            body_data.extend_from_slice(&data);
        }
    }

    let calculated_sha256sum = format!("{:x}", hasher.finalize());
    Ok((calculated_sha256sum, temp_file, body_data.freeze()))
}

async fn verify_request<H>(
    helper: &H,
    req: Request,
    sign_params: &SignParams,
) -> NotifyExchangeResult<Request>
where
    H: HttpApiCalleeAuthHelper,
{
    // Deconstruct request and extract headers
    let (mut parts, body) = req.into_parts();

    let biz_code = &sign_params.biz_code;
    let request_id = &sign_params.request_id;
    let sign_algo = sign_params.sign_algo;
    let sha256sum = get_header_for_callee_request(&parts.headers, HEADER_SHA256SUM)?;
    let signature = get_header_for_callee_request(&parts.headers, HEADER_SIGN)?;

    // Check if request_id is existed
    if helper
        .context()
        .cache_manager()
        .get_token::<RequestIdCache>(&format!("{}-{}", biz_code, request_id))
        .await?
        .is_some()
    {
        tracing::warn!("Duplicated request id: {request_id}");
        return Err(error::invalid_request("Duplicated request id"));
    };

    // Process body: stream to memory or temp file and calculate SHA256 checksum
    let (calculated_sha256sum, temp_file, body_data) = calc_body_sha256sum(
        helper.context().config().max_body_size_without_temp_file(),
        helper.context().config().temp_dir().as_deref(),
        "ne-api-callee-req-",
        body.into_data_stream(),
    )
    .await?;

    // Verify checksum
    if calculated_sha256sum != sha256sum.to_ascii_lowercase() {
        tracing::error!(
            "Checksum mismatch, expected: {sha256sum}, calculated: {calculated_sha256sum}",
        );
        return Err(error::invalid_request("Checksum mismatch"));
    }

    // Build string to sign
    let uri = if let Some(uri) = parts.extensions.get::<OriginalUri>() {
        &uri.0
    } else {
        // The `OriginalUri` extension will always be present if using
        // `Router` unless another extractor or middleware has removed it
        &parts.uri
    };

    let (sign_string, sign_headers) = build_request_sign_string(
        helper
            .context()
            .config()
            .http_auth_url_path_prefix()
            .as_deref()
            .unwrap_or_default(),
        &parts.method,
        uri,
        &parts.headers,
    )
    .map_err(error::invalid_request)?;

    // Verify signature
    helper
        .extend(&mut parts.extensions, biz_code, &parts.headers)
        .await?;
    let public_keys = helper
        .public_keys(&parts.extensions, biz_code, sign_algo)
        .await?;
    let key_index = verify(sign_algo, &public_keys, &sign_string, signature)?;
    helper
        .set_verified_public_key(&parts.extensions, key_index)
        .await?;
    parts
        .extensions
        .insert(VerifiedHeaderMap(Arc::new(sign_headers)));

    // Reconstruct request and return
    let new_body = if let Some(mut file) = temp_file {
        file.seek(SeekFrom::Start(0))
            .await
            .whatever("Failed to seek temp file")?;
        Body::from_stream(ReaderStream::new(file))
    } else {
        Body::from(body_data)
    };
    let new_req = Request::from_parts(parts, new_body);

    Ok(new_req)
}

fn sign(
    sign_algo: SignAlgo,
    private_key: &[u8],
    sign_string: &str,
) -> NotifyExchangeResult<String> {
    tracing::debug!(
        "To be signed string in hex: {}",
        hex::encode(sign_string.as_bytes())
    );
    match sign_algo {
        SignAlgo::Sha256Rsa => {
            let private_key: RsaPrivateKey = RsaPrivateKey::from_pkcs8_der(private_key)
                .whatever("Invalid private key format provided by helper")?;
            let mut signing_key = RsaSigningKey::<Sha256>::new(private_key);
            let signature = signing_key
                .try_sign(sign_string.as_bytes())
                .whatever("Fail to sign")?;
            Ok(STANDARD.encode(signature.to_vec()))
        }
    }
}

async fn sign_response<H>(
    helper: &H,
    sign_params: &SignParams,
    resp: Response,
) -> NotifyExchangeResult<Response>
where
    H: HttpApiCalleeAuthHelper,
{
    // 1. Deconstruct response
    let (mut parts, body) = resp.into_parts();

    // 2. Process body: stream to memory or temp file and calculate SHA256 checksum
    let (calculated_sha256sum, temp_file, body_data) = calc_body_sha256sum(
        helper.context().config().max_body_size_without_temp_file(),
        helper.context().config().temp_dir().as_deref(),
        "ne-api-callee-resp-",
        body.into_data_stream(),
    )
    .await?;

    // 3. Prepare headers for signing
    let response_id = Utc::now().timestamp_millis().to_string();
    let verified_headers = parts.extensions.get::<VerifiedHeaderMap>();
    let response_biz_code = if let Some(expected_biz_code) =
        verified_headers.and_then(|h| h.0.get(HEADER_EXPECTED_BIZ_CODE))
    {
        expected_biz_code
            .to_str()
            .expect("The header value should be a valid string")
            .to_string()
    } else {
        helper
            .response_biz_code(&sign_params.biz_code, sign_params.sign_algo)
            .await?
    };

    insert_header(
        &mut parts.headers,
        HEADER_ALGO,
        &sign_params.sign_algo.to_string(),
    )?;
    insert_header(&mut parts.headers, HEADER_BIZ_CODE, &response_biz_code)?;
    insert_header(&mut parts.headers, HEADER_SHA256SUM, &calculated_sha256sum)?;
    insert_header(
        &mut parts.headers,
        HEADER_REQUEST_ID,
        &sign_params.request_id,
    )?;
    insert_header(&mut parts.headers, HEADER_RESPONSE_ID, &response_id)?;

    // 4. Build string to sign
    let (sign_string, _) = build_response_sign_string(parts.status.as_u16(), &parts.headers)
        .map_err(error::internal_server_error)?;

    // 5. Get key and sign
    let private_key = helper
        .private_key(&response_biz_code, sign_params.sign_algo)
        .await?;
    let signature = sign(sign_params.sign_algo, &private_key, &sign_string)?;

    // 6. Update headers
    insert_header(&mut parts.headers, HEADER_SIGN, &signature)?;

    // 7. Reconstruct response
    let new_body = if let Some(mut file) = temp_file {
        file.seek(SeekFrom::Start(0))
            .await
            .whatever("Failed to seek temp file")?;
        Body::from_stream(ReaderStream::new(file))
    } else {
        Body::from(body_data)
    };

    let new_resp = Response::from_parts(parts, new_body);
    Ok(new_resp)
}

fn extract_sign_params(req: &Request) -> NotifyExchangeResult<SignParams> {
    let headers = req.headers();
    let sign_algo = get_header_for_callee_request(headers, HEADER_ALGO)?.parse()?;
    let biz_code = get_header_for_callee_request(headers, HEADER_BIZ_CODE)?;
    let request_id = get_header_for_callee_request(headers, HEADER_REQUEST_ID)?;
    Ok(SignParams {
        biz_code: biz_code.to_string(),
        request_id: request_id.to_string(),
        sign_algo,
    })
}

/// Try our best to return all responses with signature
async fn sign_error_response<H>(
    helper: &H,
    sign_params: &SignParams,
    error: NotifyExchangeError,
) -> Response
where
    H: HttpApiCalleeAuthHelper,
{
    let (status, header_map, response) = error.into_response_parts();
    let resp = (status, header_map.clone(), Json(response.clone())).into_response();
    match sign_response(helper, sign_params, resp).await {
        Ok(resp) => resp,
        Err(e) => {
            tracing::error!("Error happened at signing the error response: {e:#?}");
            (status, header_map, Json(response)).into_response()
        }
    }
}

pub async fn http_api_auth_middleware<H>(
    State(helper): State<H>,
    req: Request,
    next: Next,
) -> Response
where
    H: HttpApiCalleeAuthHelper,
{
    let sign_params = match extract_sign_params(&req) {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };

    let req = match verify_request(&helper, req, &sign_params).await {
        Ok(req) => req,
        Err(e) => return sign_error_response(&helper, &sign_params, e).await,
    };

    // Save request id
    if let Err(e) = helper
        .context()
        .cache_manager()
        .add_named_token(
            &format!("{}-{}", sign_params.biz_code, sign_params.request_id),
            &RequestIdCache::default(),
            Some(Duration::from_secs(24 * 3600)),
            10,
        )
        .await
    {
        return sign_error_response(&helper, &sign_params, e).await;
    }

    let verified_headers = req.extensions().get::<VerifiedHeaderMap>().cloned();
    let mut resp = next.run(req).await;

    if let Some(verified_headers) = verified_headers {
        resp.extensions_mut().insert(verified_headers);
    }

    match sign_response(&helper, &sign_params, resp).await {
        Ok(resp) => resp,
        Err(e) => e.into_response(),
    }
}

async fn sign_request<H>(
    helper: &H,
    req: &mut ReqwestRequest,
    sign_algo: SignAlgo,
    response_biz_code: Option<&str>,
) -> NotifyExchangeResult<String>
where
    H: HttpApiCallerAuthHelper,
{
    let uri: Uri = req.url().as_str().parse().map_err(|e| {
        tracing::debug!("Invalid url: {e:?}");
        error::internal_server_error("Invalid request url")
    })?;
    let method = req.method().clone();

    let request_biz_code = helper.request_biz_code().await?;
    let request_id = Uuid::now_v7().to_string();

    // 1. Process body: stream to memory or temp file and calculate SHA256 checksum
    let body_stream = if let Some(body) = req.body_mut().take() {
        DataStream(body)
    } else {
        DataStream(vec![].into())
    };
    let (calculated_sha256sum, temp_file, body_data) = calc_body_sha256sum(
        helper.max_body_size_without_temp_file(),
        helper.temp_dir(),
        "ne-api-caller-req-",
        body_stream,
    )
    .await?;

    // 2. Update headers
    let headers = req.headers_mut();
    insert_header(headers, HEADER_ALGO, &sign_algo.to_string())?;
    insert_header(headers, HEADER_BIZ_CODE, &request_biz_code)?;
    if let Some(response_biz_code) = response_biz_code {
        // Add HEADER_EXPECTED_BIZ_CODE to HEADER_ADDITIONAL_HEADERS
        let additional_headers = get_header_with_default(headers, HEADER_ADDITIONAL_HEADERS, "")
            .map_err(error::internal_server_error)?;

        let additional_headers = if additional_headers.is_empty() {
            HEADER_EXPECTED_BIZ_CODE.to_string()
        } else {
            format!("{},{}", additional_headers, HEADER_EXPECTED_BIZ_CODE)
        };
        insert_header(headers, HEADER_ADDITIONAL_HEADERS, &additional_headers)?;
        insert_header(headers, HEADER_EXPECTED_BIZ_CODE, response_biz_code)?;
    }
    insert_header(headers, HEADER_REQUEST_ID, &request_id)?;
    insert_header(headers, HEADER_SHA256SUM, &calculated_sha256sum)?;

    // 3. Build string to sign
    let (sign_string, _) = build_request_sign_string("", &method, &uri, headers)
        .map_err(error::internal_server_error)?;

    // 4. Calculate signature
    let private_key = helper.private_key().await?;
    let signature = sign(sign_algo, &private_key, &sign_string)?;
    insert_header(headers, HEADER_SIGN, &signature)?;

    // 5. Reconstruct request and return
    let new_body = if let Some(mut file) = temp_file {
        file.seek(SeekFrom::Start(0))
            .await
            .whatever("Failed to seek temp file")?;
        ReqwestBody::wrap_stream(ReaderStream::new(file))
    } else {
        body_data.into()
    };
    *req.body_mut() = Some(new_body);

    Ok(request_id)
}

async fn verify_response<H>(
    helper: &H,
    resp: ReqwestResponse,
    expected_sign_algo: SignAlgo,
    expected_biz_code: Option<&str>,
    expected_request_id: &str,
) -> NotifyExchangeResult<ReqwestResponse>
where
    H: HttpApiCallerAuthHelper,
{
    let resp: Response<ReqwestBody> = resp.into();
    let (mut parts, body) = resp.into_parts();
    let sign_algo = get_header_for_caller_response(&parts.headers, HEADER_ALGO)?.parse()?;
    let biz_code = get_header_for_caller_response(&parts.headers, HEADER_BIZ_CODE)?;
    let sha256sum = get_header_for_caller_response(&parts.headers, HEADER_SHA256SUM)?;
    let signature = get_header_for_caller_response(&parts.headers, HEADER_SIGN)?;
    let request_id = get_header_for_caller_response(&parts.headers, HEADER_REQUEST_ID)?;
    let response_id = get_header_for_caller_response(&parts.headers, HEADER_RESPONSE_ID)?;

    // Log request id and response id
    tracing::debug!("Response request_id: {request_id}, response_id: {response_id}",);

    if expected_sign_algo != sign_algo {
        tracing::error!(
            "Sign algorithm mismatch in the response, expected: {expected_sign_algo}, got: {sign_algo}",
        );
        return Err(error::invalid_sign(
            "Sign algorithm mismatch in the response",
        ));
    }
    if expected_biz_code.filter(|c| *c != biz_code).is_some() {
        tracing::error!(
            "Biz code in the response isn't the expected one, expected: {expected_biz_code:?}, got: {biz_code}",
        );
        return Err(error::invalid_sign(
            "Biz code in the response isn't the expected one",
        ));
    }
    if expected_request_id != request_id {
        tracing::error!(
            "Request ID mismatch in the response, expected: {expected_request_id}, got: {request_id}",
        );
        return Err(error::invalid_sign("Request ID mismatch in the response"));
    }

    let body_stream = DataStream(body);
    let (calculated_sha256sum, temp_file, body_data) = calc_body_sha256sum(
        helper.max_body_size_without_temp_file(),
        helper.temp_dir(),
        "ne-api-caller-resp-",
        body_stream,
    )
    .await?;

    if calculated_sha256sum != sha256sum.to_ascii_lowercase() {
        tracing::debug!(
            "Checksum mismatch in the response, expected: {sha256sum}, calculated: {calculated_sha256sum}"
        );
        return Err(error::invalid_response("Checksum mismatch"));
    }

    let (sign_string, sign_headers) =
        build_response_sign_string(parts.status.as_u16(), &parts.headers)
            .map_err(error::invalid_response)?;

    let public_keys = helper.public_keys(biz_code).await?;
    let _ = verify(sign_algo, &public_keys, &sign_string, signature)?;
    parts
        .extensions
        .insert(VerifiedHeaderMap(Arc::new(sign_headers)));

    let new_body = if let Some(mut file) = temp_file {
        file.seek(SeekFrom::Start(0))
            .await
            .whatever("Failed to seek temp file")?;
        ReqwestBody::wrap_stream(ReaderStream::new(file))
    } else {
        body_data.into()
    };

    Ok(Response::from_parts(parts, new_body).into())
}

async fn auth_api_call<H>(helper: &H, req: RequestBuilder) -> NotifyExchangeResult<ReqwestResponse>
where
    H: HttpApiCallerAuthHelper,
{
    let sign_algo = helper.sign_algo().await?;
    let response_biz_code = helper.response_biz_code().await?;

    let (client, req) = req.build_split();
    let mut req = req?;

    let request_id =
        sign_request(helper, &mut req, sign_algo, response_biz_code.as_deref()).await?;

    let resp = RequestBuilder::from_parts(client, req).send().await?;

    let resp = verify_response(
        helper,
        resp,
        sign_algo,
        response_biz_code.as_deref(),
        &request_id,
    )
    .await?;

    Ok(resp)
}

pub async fn web_session_auth_middleware(mut req: Request, next: Next) -> Response {
    let jar = match extract_and_set_web_session(&mut req).await {
        Ok(jar) => jar,
        Err(e) => return e.into_response(),
    };
    let resp = next.run(req).await;
    // Update cookies
    (jar, resp).into_response()
}

/// Extract and set the web session
///
/// # Returns
///
/// Return a `CookieJar` to update the max-age of session id.
async fn extract_and_set_web_session(req: &mut Request) -> NotifyExchangeResult<CookieJar> {
    let Ok(mut jar) = req.extract_parts::<CookieJar>().await;
    let Some(session_id) = jar.get(COOKIE_SESSION_ID) else {
        return Err(error::unauthorized(
            "No session id",
            vec![WwwAuthenticate::NeLogin],
        ));
    };
    let session_id_value = session_id.value().to_string();
    let method = req.method().clone();
    let header_csrf_token = req.headers().get(HEADER_CSRF_TOKEN).cloned();

    let Some(req_ctx) = req.extensions_mut().get_mut::<RequestContext>() else {
        tracing::error!(
            "RequestContext hasn't been created, it looks like there is something wrong in the code, url: {}",
            req.uri()
        );
        return Err(error::internal_server_error(
            "RequestContext hasn't been created, it looks like there is something wrong in the code",
        ));
    };
    let context = req_ctx.global.clone();
    let Some(mut session) = context
        .cache_manager()
        .get_token::<SessionCache>(&session_id_value)
        .await?
    else {
        return Err(error::unauthorized(
            "Invalid session id",
            vec![WwwAuthenticate::NeLogin],
        ));
    };

    // Check csrf token in `DELETE`, `PATCH`, `POST` and `PUT` requests
    if (method == Method::DELETE
        || method == Method::PATCH
        || method == Method::POST
        || method == Method::PUT)
        && header_csrf_token
            .filter(|h| h.as_bytes() == session.csrf_token.as_bytes())
            .is_none()
    {
        tracing::warn!(
            "Request with method[{}] has no csrf_token or invalid one",
            method
        );
        return Err(error::unauthorized(
            "Invalid csrf token",
            vec![WwwAuthenticate::NeLogin],
        ));
    }

    // Check if session is expired
    let now = Utc::now();
    if session.expired_at < now {
        context
            .cache_manager()
            .delete_token::<SessionCache>(&session_id_value)
            .await?;
        return Err(error::unauthorized(
            "Session is expired",
            vec![WwwAuthenticate::NeLogin],
        ));
    }

    // Update the user info if the cache is fetched before 2 minutes
    if session.updated_at < now - Duration::from_secs(120) {
        let timeout = SESSION_TIMEOUT;
        session.expired_at += timeout;
        session.updated_at = now;
        let Some((user, roles)) = context
            .user_service()
            .find_user_and_roles_by_id(context.db(), session.user.id)
            .await?
        else {
            return Err(error::unauthorized(
                "User is deleted",
                vec![WwwAuthenticate::NeLogin],
            ));
        };
        if !user.enabled {
            return Err(error::unauthorized(
                "User is disabled",
                vec![WwwAuthenticate::NeLogin],
            ));
        }
        session.user = user;
        session.roles = roles;
        // Update session
        context
            .cache_manager()
            .set_named_token(session_id.value(), &session, Some(timeout))
            .await?;
        // Update the max age of session id
        let session_id = session_id.to_owned();
        jar = jar.add(
            Cookie::build(session_id)
                .path("/")
                .max_age(timeout.try_into().whatever("Invalid time range")?)
                .http_only(true)
                .secure(true),
        );
    }

    // Set the id, so we can get session id directly.
    session.id = session_id_value;
    req_ctx.user_session = Some(Arc::new(session));
    Ok(jar)
}

pub async fn web_session_once_auth_middleware(mut req: Request, next: Next) -> Response {
    let jar = match extract_and_set_web_session(&mut req).await {
        Ok(jar) => jar,
        Err(e) => return e.into_response(),
    };
    let resp = next.run(req).await;
    // Update cookies
    (jar, resp).into_response()
}

/// Verify the init_data from a telegram mini app login.
///
/// # Returns
///
/// Return the user if the verification is successful.
#[tracing::instrument(level = "debug", skip_all, err, ret)]
pub async fn verify_telegram_login(
    context: &Context,
    init_data: &str,
    endpoint_id: Id,
) -> NotifyExchangeResult<User> {
    tracing::debug!("init_data: {init_data}, endpoint_id: {endpoint_id}");
    let telegram_service = TelegramService::new(context);

    // Fetch endpoint and related user/transport info
    let endpoint = context
        .endpoint_service()
        .find_by_id(context.db(), endpoint_id)
        .await?
        .ok_or_else(|| error::invalid_request("Endpoint not found"))?;

    if endpoint.transport_service_type != TransportServiceType::Telegram {
        tracing::warn!(
            "An endpoint of [{:?}] is used to telegram login",
            endpoint.transport_service_type
        );
        return Err(error::invalid_request("Endpoint not found"));
    }

    // Check if transport service is enabled
    let _ = context
        .transport_service_service()
        .find_by_id(context.db(), endpoint.transport_service_id)
        .await?
        .filter(|s| s.enabled)
        .ok_or_else(|| error::invalid_request("TransportService is disabled"))?;

    let secret = telegram_service
        .find_token_string(context.db(), endpoint.transport_service_id)
        .await?;

    let tmp_uri = Uri::builder()
        .path_and_query(format!("/?{}", init_data))
        .build()
        .map_err(|_| {
            tracing::debug!("Invalid init_data: {init_data}");
            error::invalid_request("Invalid init_data")
        })?;
    // Verify init_data
    let Query(mut params): Query<HashMap<String, String>> =
        Query::try_from_uri(&tmp_uri).map_err(|e| {
            tracing::debug!("Invalid init_data: {init_data}, {e:?}");
            error::invalid_request("Invalid init_data")
        })?;
    let hash = params
        .remove("hash")
        .ok_or_else(|| error::invalid_request("No hash in init_data"))?;

    // Check auth_date
    let auth_date_str = params
        .get("auth_date")
        .ok_or_else(|| error::invalid_request("No auth_date in init_data"))?;
    let auth_date = auth_date_str
        .parse::<u64>()
        .map_err(|_| error::invalid_request("Invalid auth_date"))?;
    let current_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .whatever("Unable to get current timestamp")?
        .as_secs();

    if current_ts.saturating_sub(auth_date) > 300 {
        // 5 minutes
        return Err(error::invalid_request("Login data expired"));
    }

    // Verify hash
    let mut mac = Hmac::<Sha256>::new_from_slice(b"WebAppData")
        .whatever("HMAC-SHA256 key generation failed")?;
    mac.update(secret.as_bytes());
    let key = mac.finalize().into_bytes();

    let mut data_check_string_parts: Vec<String> =
        params.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
    data_check_string_parts.sort();
    let data_check_string = data_check_string_parts.join("\n");

    let mut mac =
        Hmac::<Sha256>::new_from_slice(&key).whatever("HMAC-SHA256 data hashing failed")?;
    mac.update(data_check_string.as_bytes());

    if hex::encode(mac.finalize().into_bytes()) != hash {
        return Err(error::invalid_request("Invalid hash"));
    }

    // Verify endpoint_id (user)
    let user_json_str = params
        .get("user")
        .ok_or_else(|| error::invalid_request("No user in init_data"))?;

    #[derive(Deserialize)]
    struct InitDataUser {
        id: UserId,
    }

    let tg_user: InitDataUser = serde_json::from_str(user_json_str)
        .map_err(|_| error::invalid_request("Invalid user data"))?;
    if endpoint.code != telegram::user_endpoint_code(tg_user.id) {
        return Err(error::invalid_request("Mismatched user"));
    }

    // Get user from our DB
    context
        .user_service()
        .find_user_by_id(context.db(), endpoint.user_id)
        .await
}

async fn check_user_role(
    req: &mut Request,
    role_codes: &HashSet<&'static str>,
) -> NotifyExchangeResult<()> {
    let Some(req_ctx) = req.extensions_mut().get_mut::<RequestContext>() else {
        tracing::error!(
            "RequestContext hasn't been created, it looks like there is something wrong in the code, url: {}",
            req.uri()
        );
        return Err(error::internal_server_error(
            "RequestContext hasn't been created, it looks like there is something wrong in the code",
        ));
    };

    let Some(user_session) = &req_ctx.user_session else {
        return Err(error::internal_server_error(
            "No user session found, did dev add `web_session_auth_middleware`?",
        ));
    };

    tracing::debug!(
        "Checking if the user[{}] contains one of these roles: [{role_codes:?}]",
        user_session.user.id
    );

    if user_session
        .roles
        .iter()
        .any(|r| role_codes.contains(r.code.as_str()))
    {
        Ok(())
    } else {
        Err(error::forbidden("Not enough permission"))
    }
}

pub async fn check_user_role_middleware(
    State(role_codes): State<Arc<HashSet<&'static str>>>,
    mut req: Request,
    next: Next,
) -> Response {
    if let Err(e) = check_user_role(&mut req, &role_codes).await {
        return e.into_response();
    }
    next.run(req).await
}
