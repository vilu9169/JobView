//! Installed desktop OAuth. Tokens never cross IPC or enter the database.
//! Endpoints are fixed in production; loopback and HTTP injection are private
//! test seams. No request URL, response body, or provider error is logged.

use std::collections::HashSet;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use reqwest::{redirect::Policy, Client, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, Mutex};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use super::credentials::{CredentialError, SecretValue, SecureCredentialService};

pub const GMAIL_READONLY_SCOPE: &str = "https://www.googleapis.com/auth/gmail.readonly";
const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const CALLBACK_PATH: &str = "/oauth2/callback";
const CLIENT_KEY: &str = "desktop-client-v1";
const TOKEN_KEY: &str = "refresh-token-v1";
const MAX_CALLBACK_BYTES: usize = 8192;
const MAX_TOKEN_BYTES: usize = 16384;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(180);
const EXPIRY_MARGIN: u64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OAuthError {
    #[error("Import a Google Desktop app OAuth client JSON file in Settings first.")]
    NotConfigured,
    #[error("Choose the downloaded Google OAuth JSON for a Desktop app with Google endpoints.")]
    InvalidConfiguration,
    #[error("Disconnect Gmail before replacing its OAuth client configuration.")]
    AlreadyConnected,
    #[error("A Gmail connection operation is already in progress.")]
    Busy,
    #[error("Windows secure credential storage is unavailable. Gmail credentials were not saved.")]
    CredentialStore,
    #[error("Stored Gmail credentials are invalid. Import the client setup again and reconnect.")]
    InvalidCredentials,
    #[error("The system browser could not be opened. Check your default browser and try again.")]
    BrowserUnavailable,
    #[error("The local Gmail sign-in listener could not be started. Try connecting again.")]
    ListenerUnavailable,
    #[error("Gmail sign-in timed out. Choose Connect Gmail to try again.")]
    TimedOut,
    #[error("Gmail sign-in was cancelled.")]
    Cancelled,
    #[error("Google sign-in was declined or could not be completed. Try connecting again.")]
    AccessDenied,
    #[error("Google returned an invalid sign-in response. Try connecting again.")]
    InvalidResponse,
    #[error(
        "Google did not grant exactly the requested read-only Gmail permission. Reconnect Gmail."
    )]
    InvalidScope,
    #[error("Google could not be reached. Check your connection and try again.")]
    Network,
    #[error(
        "Google could not complete authorization. Check the Desktop client setup and try again."
    )]
    AuthorizationFailed,
    #[error("Gmail authorization expired or was revoked. Reconnect Gmail in Settings.")]
    ReconnectRequired,
    #[error("Secure random values could not be generated. Try connecting again.")]
    RandomUnavailable,
}

impl OAuthError {
    pub fn requires_reconnect(&self) -> bool {
        matches!(
            self,
            Self::ReconnectRequired | Self::InvalidCredentials | Self::InvalidScope
        )
    }
}

impl From<CredentialError> for OAuthError {
    fn from(_: CredentialError) -> Self {
        Self::CredentialStore
    }
}

#[derive(Debug, Clone)]
pub struct ClientSummary {
    pub client_id: String,
}

#[derive(Deserialize)]
struct ClientDownload {
    installed: Option<InstalledClient>,
    web: Option<serde::de::IgnoredAny>,
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
struct InstalledClient {
    client_id: String,
    #[serde(default)]
    client_secret: Option<String>,
    auth_uri: String,
    token_uri: String,
    redirect_uris: Vec<String>,
}

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
struct ClientConfiguration {
    client_id: String,
    client_secret: Option<String>,
}

// Access tokens stay only in RAM. Storing the refresh token separately avoids
// Windows Credential Manager's per-entry blob limit even at Google's max token
// sizes (an access token alone can be 2048 bytes).
#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
struct RefreshCredential {
    refresh_token: String,
    refresh_expires_at: Option<u64>,
}

#[derive(Zeroize, ZeroizeOnDrop)]
struct AccessCredential {
    access_token: String,
    expires_at: u64,
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
struct TokenResponse {
    access_token: String,
    token_type: String,
    expires_in: u64,
    refresh_token: Option<String>,
    refresh_token_expires_in: Option<u64>,
    scope: Option<String>,
}

trait BrowserOpener: Send + Sync {
    fn open(&self, url: &str) -> Result<(), OAuthError>;
}

struct SystemBrowser;
impl BrowserOpener for SystemBrowser {
    fn open(&self, url: &str) -> Result<(), OAuthError> {
        // The Windows shell opens the user's system browser directly, without
        // a command interpreter or browser helper that logs the OAuth URL.
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Com::{
                CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
            };
            use windows_sys::Win32::UI::{
                Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL,
            };
            let url = Zeroizing::new(url.encode_utf16().chain(Some(0)).collect::<Vec<_>>());
            let verb: [u16; 5] = [b'o' as u16, b'p' as u16, b'e' as u16, b'n' as u16, 0];
            // SAFETY: verb and URL are live, NUL-terminated UTF-16 allocations;
            // optional parameters are null. The only caller builds a fixed
            // HTTPS Google authorization URL. The returned value is a status,
            // not an owned OS handle.
            let result = unsafe {
                // Some protocol handlers require COM. If this thread already
                // has a different apartment, leave that initialization intact.
                let initialized = CoInitializeEx(
                    std::ptr::null(),
                    (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
                ) >= 0;
                let result = ShellExecuteW(
                    std::ptr::null_mut(),
                    verb.as_ptr(),
                    url.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    SW_SHOWNORMAL,
                );
                if initialized {
                    CoUninitialize();
                }
                result
            };
            if result as isize > 32 {
                Ok(())
            } else {
                Err(OAuthError::BrowserUnavailable)
            }
        }
        #[cfg(not(windows))]
        {
            let _ = url;
            Err(OAuthError::BrowserUnavailable)
        }
    }
}

pub struct OAuthService {
    store: Arc<dyn SecureCredentialService>,
    client: Client,
    operation: Mutex<()>,
    access: Mutex<Option<AccessCredential>>,
    cancellation: StdMutex<Option<watch::Sender<bool>>>,
    browser: Arc<dyn BrowserOpener>,
    #[cfg(test)]
    token_endpoint: Option<String>,
    #[cfg(test)]
    connect_timeout: Duration,
}

impl OAuthService {
    pub fn new(store: Arc<dyn SecureCredentialService>) -> Result<Self, OAuthError> {
        let client = Client::builder()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("JobView/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| OAuthError::Network)?;
        Ok(Self {
            store,
            client,
            operation: Mutex::new(()),
            access: Mutex::new(None),
            cancellation: StdMutex::new(None),
            browser: Arc::new(SystemBrowser),
            #[cfg(test)]
            token_endpoint: None,
            #[cfg(test)]
            connect_timeout: CONNECT_TIMEOUT,
        })
    }

    pub async fn configure(&self, json: &str) -> Result<(), OAuthError> {
        let _operation = self.operation.try_lock().map_err(|_| OAuthError::Busy)?;
        if self.store.get(TOKEN_KEY)?.is_some() {
            return Err(OAuthError::AlreadyConnected);
        }
        let configuration = parse_configuration(json)?;
        let bytes =
            serde_json::to_vec(&configuration).map_err(|_| OAuthError::InvalidConfiguration)?;
        self.store.set(CLIENT_KEY, &SecretValue::new(bytes))?;
        Ok(())
    }

    pub async fn configuration(&self) -> Result<Option<ClientSummary>, OAuthError> {
        self.load_configuration().map(|configuration| {
            configuration.map(|value| ClientSummary {
                client_id: value.client_id.clone(),
            })
        })
    }

    pub async fn has_tokens(&self) -> Result<bool, OAuthError> {
        let credential = self.load_refresh()?;
        if credential.as_ref().is_some_and(refresh_expired) {
            return Err(OAuthError::ReconnectRequired);
        }
        Ok(credential.is_some())
    }

    pub async fn connect(&self) -> Result<(), OAuthError> {
        let _operation = self.operation.try_lock().map_err(|_| OAuthError::Busy)?;
        let (sender, mut receiver) = watch::channel(false);
        *self
            .cancellation
            .lock()
            .map_err(|_| OAuthError::Cancelled)? = Some(sender);
        let _cleanup = CancellationCleanup(&self.cancellation);
        let timeout = {
            #[cfg(test)]
            {
                self.connect_timeout
            }
            #[cfg(not(test))]
            {
                CONNECT_TIMEOUT
            }
        };
        tokio::select! {
            biased;
            _ = receiver.changed() => Err(OAuthError::Cancelled),
            result = tokio::time::timeout(timeout, self.connect_inner()) => {
                result.map_err(|_| OAuthError::TimedOut)?
            }
        }
    }

    pub fn cancel(&self) {
        if let Ok(cancellation) = self.cancellation.lock() {
            if let Some(sender) = cancellation.as_ref() {
                let _ = sender.send(true);
            }
        }
    }

    pub async fn access_token(&self, force_refresh: bool) -> Result<SecretValue, OAuthError> {
        let _operation = self.operation.lock().await;
        let refresh = self.load_refresh()?.ok_or(OAuthError::ReconnectRequired)?;
        if refresh_expired(&refresh) {
            self.clear_tokens().await?;
            return Err(OAuthError::ReconnectRequired);
        }
        if !force_refresh {
            let access = self.access.lock().await;
            if let Some(value) = access.as_ref() {
                if value.expires_at > now().saturating_add(EXPIRY_MARGIN) {
                    return Ok(SecretValue::new(value.access_token.as_bytes().to_vec()));
                }
            }
        }
        let configuration = self
            .load_configuration()?
            .ok_or(OAuthError::NotConfigured)?;
        let mut parameters = vec![
            ("client_id", configuration.client_id.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh.refresh_token.as_str()),
        ];
        if let Some(secret) = configuration.client_secret.as_deref() {
            parameters.push(("client_secret", secret));
        }
        let response = self.request_token(&parameters).await;
        let response = match response {
            Err(error) if error.requires_reconnect() => {
                self.clear_tokens().await?;
                return Err(error);
            }
            other => other?,
        };
        self.save_tokens(response, Some(&refresh)).await?;
        let access = self.access.lock().await;
        let value = access.as_ref().ok_or(OAuthError::ReconnectRequired)?;
        Ok(SecretValue::new(value.access_token.as_bytes().to_vec()))
    }

    pub async fn disconnect(&self) -> Result<(), OAuthError> {
        self.cancel();
        let _operation = self.operation.lock().await;
        self.clear_tokens().await
    }

    fn load_configuration(&self) -> Result<Option<ClientConfiguration>, OAuthError> {
        self.store
            .get(CLIENT_KEY)?
            .map(|secret| {
                let configuration: ClientConfiguration = serde_json::from_slice(secret.expose())
                    .map_err(|_| OAuthError::InvalidCredentials)?;
                validate_client_values(
                    &configuration.client_id,
                    configuration.client_secret.as_deref(),
                )
                .map_err(|_| OAuthError::InvalidCredentials)?;
                Ok(configuration)
            })
            .transpose()
    }

    fn load_refresh(&self) -> Result<Option<RefreshCredential>, OAuthError> {
        self.store
            .get(TOKEN_KEY)?
            .map(|secret| {
                let credential: RefreshCredential = serde_json::from_slice(secret.expose())
                    .map_err(|_| OAuthError::InvalidCredentials)?;
                if !valid_token(&credential.refresh_token, 1024) {
                    return Err(OAuthError::InvalidCredentials);
                }
                Ok(credential)
            })
            .transpose()
    }

    async fn clear_tokens(&self) -> Result<(), OAuthError> {
        *self.access.lock().await = None;
        self.store.delete(TOKEN_KEY)?;
        Ok(())
    }

    async fn connect_inner(&self) -> Result<(), OAuthError> {
        let configuration = self
            .load_configuration()?
            .ok_or(OAuthError::NotConfigured)?;
        match self.load_refresh() {
            Ok(Some(refresh)) if refresh_expired(&refresh) => self.clear_tokens().await?,
            Err(OAuthError::InvalidCredentials) => self.clear_tokens().await?,
            Ok(Some(_)) => return Err(OAuthError::AlreadyConnected),
            Ok(None) => {}
            Err(error) => return Err(error),
        }
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| OAuthError::ListenerUnavailable)?;
        let address = listener
            .local_addr()
            .map_err(|_| OAuthError::ListenerUnavailable)?;
        let redirect_uri = format!("http://{address}{CALLBACK_PATH}");
        let state = random_secret()?;
        let verifier = random_secret()?;
        let url = authorization_url(&configuration.client_id, &redirect_uri, &state, &verifier)?;
        self.browser.open(url.as_str())?;
        let code = receive_callback(&listener, &address.to_string(), &state).await?;
        // Close the listener as soon as one valid response has been received.
        drop(listener);
        let mut parameters = vec![
            ("client_id", configuration.client_id.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri.as_str()),
            ("code", code.as_str()),
            ("code_verifier", verifier.as_str()),
        ];
        if let Some(secret) = configuration.client_secret.as_deref() {
            parameters.push(("client_secret", secret));
        }
        let response = self.request_token(&parameters).await?;
        self.save_tokens(response, None).await
    }

    async fn request_token(
        &self,
        parameters: &[(&str, &str)],
    ) -> Result<TokenResponse, OAuthError> {
        let endpoint = {
            #[cfg(test)]
            {
                self.token_endpoint.as_deref().unwrap_or(TOKEN_URL)
            }
            #[cfg(not(test))]
            {
                TOKEN_URL
            }
        };
        let mut response = self
            .client
            .post(endpoint)
            .form(parameters)
            .send()
            .await
            .map_err(|_| OAuthError::Network)?;
        let status = response.status();
        if response
            .content_length()
            .is_some_and(|size| size > MAX_TOKEN_BYTES as u64)
        {
            return Err(OAuthError::InvalidResponse);
        }
        let mut body = Zeroizing::new(Vec::new());
        while let Some(chunk) = response.chunk().await.map_err(|_| OAuthError::Network)? {
            if body.len().saturating_add(chunk.len()) > MAX_TOKEN_BYTES {
                return Err(OAuthError::InvalidResponse);
            }
            body.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            #[derive(Deserialize)]
            struct Failure<'a> {
                error: &'a str,
            }
            let failure = serde_json::from_slice::<Failure<'_>>(&body).ok();
            return Err(match failure.map(|value| value.error) {
                Some("invalid_grant" | "invalid_token") => OAuthError::ReconnectRequired,
                _ => OAuthError::AuthorizationFailed,
            });
        }
        let response: TokenResponse =
            serde_json::from_slice(&body).map_err(|_| OAuthError::InvalidResponse)?;
        validate_token_response(&response)?;
        Ok(response)
    }

    async fn save_tokens(
        &self,
        response: TokenResponse,
        previous: Option<&RefreshCredential>,
    ) -> Result<(), OAuthError> {
        // Validate again at this boundary so test adapters cannot bypass scope,
        // token-type, lifetime, or blank-token checks.
        validate_token_response(&response)?;
        let refresh_token = response
            .refresh_token
            .as_deref()
            .or_else(|| previous.map(|value| value.refresh_token.as_str()))
            .ok_or(OAuthError::ReconnectRequired)?;
        let refresh_expires_at = response
            .refresh_token_expires_in
            .map(|seconds| now().saturating_add(seconds))
            .or_else(|| previous.and_then(|value| value.refresh_expires_at));
        let refresh = RefreshCredential {
            refresh_token: refresh_token.to_owned(),
            refresh_expires_at,
        };
        let encoded = serde_json::to_vec(&refresh).map_err(|_| OAuthError::InvalidResponse)?;
        // Acquire the async lock before the final synchronous commit. Cancelling
        // connect cannot interrupt between durable storage and the RAM update.
        let mut access = self.access.lock().await;
        self.store.set(TOKEN_KEY, &SecretValue::new(encoded))?;
        *access = Some(AccessCredential {
            access_token: response.access_token.clone(),
            expires_at: now().saturating_add(response.expires_in),
        });
        Ok(())
    }
}

struct CancellationCleanup<'a>(&'a StdMutex<Option<watch::Sender<bool>>>);
impl Drop for CancellationCleanup<'_> {
    fn drop(&mut self) {
        if let Ok(mut cancellation) = self.0.lock() {
            *cancellation = None;
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn refresh_expired(credential: &RefreshCredential) -> bool {
    credential
        .refresh_expires_at
        .is_some_and(|expiry| expiry <= now())
}

fn valid_token(token: &str, max_len: usize) -> bool {
    !token.is_empty() && token.len() <= max_len && token.bytes().all(|byte| byte.is_ascii_graphic())
}

fn validate_token_response(response: &TokenResponse) -> Result<(), OAuthError> {
    if !response.token_type.eq_ignore_ascii_case("Bearer")
        || !valid_token(&response.access_token, 4096)
        || response.expires_in == 0
        || response.expires_in > 86400
        || response
            .refresh_token
            .as_ref()
            .is_some_and(|value| !valid_token(value, 1024))
        || response.refresh_token_expires_in == Some(0)
    {
        return Err(OAuthError::InvalidResponse);
    }
    if let Some(scope) = response.scope.as_deref() {
        let scopes: Vec<_> = scope.split_whitespace().collect();
        if scopes != [GMAIL_READONLY_SCOPE] {
            return Err(OAuthError::InvalidScope);
        }
    }
    Ok(())
}

fn validate_client_values(client_id: &str, client_secret: Option<&str>) -> Result<(), OAuthError> {
    if client_id.len() > 512
        || !client_id.ends_with(".apps.googleusercontent.com")
        || client_id.len() <= ".apps.googleusercontent.com".len()
        || !client_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        || client_secret.is_some_and(|secret| !valid_token(secret, 1024))
    {
        return Err(OAuthError::InvalidConfiguration);
    }
    Ok(())
}

fn parse_configuration(json: &str) -> Result<ClientConfiguration, OAuthError> {
    if json.len() > 65536 {
        return Err(OAuthError::InvalidConfiguration);
    }
    let download: ClientDownload =
        serde_json::from_str(json).map_err(|_| OAuthError::InvalidConfiguration)?;
    if download.web.is_some() {
        return Err(OAuthError::InvalidConfiguration);
    }
    let installed = download.installed.ok_or(OAuthError::InvalidConfiguration)?;
    validate_client_values(&installed.client_id, installed.client_secret.as_deref())?;
    if !matches!(
        installed.auth_uri.as_str(),
        AUTH_URL | "https://accounts.google.com/o/oauth2/auth"
    ) || installed.token_uri != TOKEN_URL
        || installed.redirect_uris.is_empty()
        || !installed.redirect_uris.iter().all(|value| {
            Url::parse(value).is_ok_and(|url| {
                url.scheme() == "http"
                    && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.query().is_none()
                    && url.fragment().is_none()
                    && url.path() == "/"
            })
        })
    {
        return Err(OAuthError::InvalidConfiguration);
    }
    Ok(ClientConfiguration {
        client_id: installed.client_id.clone(),
        client_secret: installed.client_secret.clone(),
    })
}

fn random_secret() -> Result<Zeroizing<String>, OAuthError> {
    let mut random = Zeroizing::new([0u8; 32]);
    getrandom::fill(random.as_mut()).map_err(|_| OAuthError::RandomUnavailable)?;
    Ok(Zeroizing::new(URL_SAFE_NO_PAD.encode(random.as_ref())))
}

fn authorization_url(
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    verifier: &str,
) -> Result<Url, OAuthError> {
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut url = Url::parse(AUTH_URL).map_err(|_| OAuthError::InvalidConfiguration)?;
    url.query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", GMAIL_READONLY_SCOPE)
        .append_pair("state", state)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent select_account");
    Ok(url)
}

enum Callback {
    Code(Zeroizing<String>),
    Denied,
}

fn parse_callback(
    request: &[u8],
    expected_host: &str,
    expected_state: &str,
) -> Result<Callback, OAuthError> {
    let request = std::str::from_utf8(request).map_err(|_| OAuthError::InvalidResponse)?;
    let (headers, body) = request
        .split_once("\r\n\r\n")
        .ok_or(OAuthError::InvalidResponse)?;
    if request.len() > MAX_CALLBACK_BYTES || !body.is_empty() {
        return Err(OAuthError::InvalidResponse);
    }
    let mut lines = headers.split("\r\n");
    let mut first = lines.next().ok_or(OAuthError::InvalidResponse)?.split(' ');
    if first.next() != Some("GET") {
        return Err(OAuthError::InvalidResponse);
    }
    let target = first.next().ok_or(OAuthError::InvalidResponse)?;
    if first.next() != Some("HTTP/1.1")
        || first.next().is_some()
        || !target.starts_with(&format!("{CALLBACK_PATH}?"))
        || target.contains('#')
    {
        return Err(OAuthError::InvalidResponse);
    }
    let mut host_seen = false;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(OAuthError::InvalidResponse)?;
        if name.eq_ignore_ascii_case("host") {
            if host_seen || value.trim() != expected_host {
                return Err(OAuthError::InvalidResponse);
            }
            host_seen = true;
        }
        if name.eq_ignore_ascii_case("transfer-encoding")
            || (name.eq_ignore_ascii_case("content-length") && value.trim() != "0")
        {
            return Err(OAuthError::InvalidResponse);
        }
    }
    if !host_seen {
        return Err(OAuthError::InvalidResponse);
    }
    let url = Url::parse(&format!("http://{expected_host}{target}"))
        .map_err(|_| OAuthError::InvalidResponse)?;
    if url.path() != CALLBACK_PATH {
        return Err(OAuthError::InvalidResponse);
    }
    let mut names = HashSet::new();
    let mut state = None;
    let mut code = None;
    let mut error = false;
    for (name, value) in url.query_pairs() {
        if !names.insert(name.to_string()) {
            return Err(OAuthError::InvalidResponse);
        }
        match name.as_ref() {
            "state" => state = Some(Zeroizing::new(value.into_owned())),
            "code" => code = Some(Zeroizing::new(value.into_owned())),
            "error" => error = true,
            _ => {}
        }
    }
    if !state
        .as_deref()
        .is_some_and(|state| constant_time_equal(state.as_bytes(), expected_state.as_bytes()))
    {
        return Err(OAuthError::InvalidResponse);
    }
    match (code, error) {
        (Some(code), false) if valid_token(&code, 2048) => Ok(Callback::Code(code)),
        (None, true) => Ok(Callback::Denied),
        _ => Err(OAuthError::InvalidResponse),
    }
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

async fn read_callback(stream: &mut TcpStream) -> Result<Zeroizing<Vec<u8>>, OAuthError> {
    let mut request = Zeroizing::new(Vec::with_capacity(1024));
    let mut buffer = Zeroizing::new([0u8; 1024]);
    loop {
        let count = stream
            .read(buffer.as_mut())
            .await
            .map_err(|_| OAuthError::InvalidResponse)?;
        if count == 0 || request.len() + count > MAX_CALLBACK_BYTES {
            return Err(OAuthError::InvalidResponse);
        }
        request.extend_from_slice(&buffer[..count]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(request);
        }
    }
}

async fn callback_reply(stream: &mut TcpStream, accepted: bool) {
    let (status, body) = if accepted {
        (
            "200 OK",
            "Gmail sign-in response received. Return to JobView to check the connection.",
        )
    } else {
        (
            "400 Bad Request",
            "This sign-in response could not be accepted. Return to JobView.",
        )
    };
    let response = format!("HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; frame-ancestors 'none'\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n{body}", body.len());
    let _ = tokio::time::timeout(
        Duration::from_secs(1),
        stream.write_all(response.as_bytes()),
    )
    .await;
}

async fn receive_callback(
    listener: &TcpListener,
    host: &str,
    state: &str,
) -> Result<Zeroizing<String>, OAuthError> {
    loop {
        let (mut stream, peer) = listener
            .accept()
            .await
            .map_err(|_| OAuthError::ListenerUnavailable)?;
        if !peer.ip().is_loopback() {
            continue;
        }
        let callback =
            match tokio::time::timeout(Duration::from_secs(3), read_callback(&mut stream)).await {
                Ok(Ok(request)) => parse_callback(&request, host, state),
                _ => Err(OAuthError::InvalidResponse),
            };
        callback_reply(&mut stream, callback.is_ok()).await;
        match callback {
            Ok(Callback::Code(code)) => return Ok(code),
            Ok(Callback::Denied) => return Err(OAuthError::AccessDenied),
            Err(_) => continue,
        }
    }
}

#[cfg(test)]
mod tests;
