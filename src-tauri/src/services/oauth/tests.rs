use super::*;
use crate::services::credentials::{MemoryCredentialService, UnavailableCredentialService};
use tokio::sync::mpsc;

const CLIENT_JSON: &str = r#"{"installed":{"client_id":"fictional.apps.googleusercontent.com","client_secret":"fictional-client-secret","auth_uri":"https://accounts.google.com/o/oauth2/auth","token_uri":"https://oauth2.googleapis.com/token","redirect_uris":["http://localhost"]}}"#;

struct CaptureBrowser(mpsc::UnboundedSender<String>);
impl BrowserOpener for CaptureBrowser {
    fn open(&self, url: &str) -> Result<(), OAuthError> {
        self.0
            .send(url.to_owned())
            .map_err(|_| OAuthError::BrowserUnavailable)
    }
}

fn test_service() -> (
    OAuthService,
    Arc<MemoryCredentialService>,
    mpsc::UnboundedReceiver<String>,
) {
    let store = Arc::new(MemoryCredentialService::default());
    let (sender, receiver) = mpsc::unbounded_channel();
    let mut service = OAuthService::new(store.clone()).unwrap();
    service.browser = Arc::new(CaptureBrowser(sender));
    (service, store, receiver)
}

fn token_response(access: &str, refresh: Option<&str>) -> TokenResponse {
    TokenResponse {
        access_token: access.to_owned(),
        token_type: "Bearer".to_owned(),
        expires_in: 3600,
        refresh_token: refresh.map(str::to_owned),
        refresh_token_expires_in: None,
        scope: Some(GMAIL_READONLY_SCOPE.to_owned()),
    }
}

fn callback_request(target: &str) -> String {
    format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1:12345\r\n\r\n")
}

async fn token_server(status: u16, body: String) -> (String, tokio::task::JoinHandle<String>) {
    token_server_headers(status, body, "").await
}

async fn token_server_headers(
    status: u16,
    body: String,
    extra_headers: &str,
) -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/token", listener.local_addr().unwrap());
    let extra_headers = extra_headers.to_owned();
    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0u8; 2048];
        loop {
            let count = stream.read(&mut buffer).await.unwrap();
            assert_ne!(count, 0);
            request.extend_from_slice(&buffer[..count]);
            let text = std::str::from_utf8(&request).unwrap();
            if let Some((headers, body)) = text.split_once("\r\n\r\n") {
                let content_length: usize = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().unwrap())
                    })
                    .unwrap_or_default();
                if body.len() >= content_length {
                    break;
                }
            }
        }
        let response = format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra_headers}Connection: close\r\n\r\n{body}", body.len());
        stream.write_all(response.as_bytes()).await.unwrap();
        String::from_utf8(request).unwrap()
    });
    (endpoint, handle)
}

async fn browser_callback(url: &str, state_override: Option<&str>) -> String {
    let authorization = Url::parse(url).unwrap();
    let query: std::collections::HashMap<_, _> = authorization.query_pairs().into_owned().collect();
    let mut callback = Url::parse(&query["redirect_uri"]).unwrap();
    callback
        .query_pairs_mut()
        .append_pair("state", state_override.unwrap_or(&query["state"]))
        .append_pair("code", "fictional-auth-code");
    let host = format!("127.0.0.1:{}", callback.port().unwrap());
    let mut stream = TcpStream::connect(&host).await.unwrap();
    let request = format!(
        "GET {}?{} HTTP/1.1\r\nHost: {host}\r\n\r\n",
        callback.path(),
        callback.query().unwrap()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    response
}

#[tokio::test]
async fn imports_only_desktop_google_configuration_and_summary_has_no_secret() {
    let (service, store, _) = test_service();
    assert!(service.configuration().await.unwrap().is_none());
    service.configure(CLIENT_JSON).await.unwrap();
    let summary = service.configuration().await.unwrap().unwrap();
    assert_eq!(summary.client_id, "fictional.apps.googleusercontent.com");
    assert!(!format!("{summary:?}").contains("fictional-client-secret"));
    assert!(store.get(CLIENT_KEY).unwrap().is_some());
    for invalid in [
        CLIENT_JSON.replace("installed", "web"),
        CLIENT_JSON.replace(
            "https://oauth2.googleapis.com/token",
            "https://evil.invalid/token",
        ),
        CLIENT_JSON.replace(
            "https://accounts.google.com/o/oauth2/auth",
            "https://accounts.google.com.evil.invalid/auth",
        ),
        CLIENT_JSON.replace("http://localhost", "https://evil.invalid"),
        CLIENT_JSON.replace("http://localhost", "http://user@localhost"),
        CLIENT_JSON.replace("fictional.apps.googleusercontent.com", "bad-client"),
        "not-json-fictional-secret".to_owned(),
    ] {
        assert_eq!(
            service.configure(&invalid).await,
            Err(OAuthError::InvalidConfiguration)
        );
    }
}

#[tokio::test]
async fn unavailable_storage_never_falls_back_to_memory_or_file() {
    let service = OAuthService::new(Arc::new(UnavailableCredentialService)).unwrap();
    assert_eq!(
        service.configure(CLIENT_JSON).await,
        Err(OAuthError::CredentialStore)
    );
    assert!(matches!(
        service.configuration().await,
        Err(OAuthError::CredentialStore)
    ));
    assert_eq!(service.has_tokens().await, Err(OAuthError::CredentialStore));
}

#[test]
fn callback_rejects_state_host_path_method_duplicates_and_ambiguous_responses() {
    for target in [
        "/oauth2/callback?code=secret",
        "/oauth2/callback?state=wrong&code=secret",
        "/oauth2/callback?state=correct&state=wrong&code=secret",
        "/oauth2/callback?state=correct&code=secret&code=second",
        "/oauth2/callback?state=correct&code=secret&error=access_denied",
        "/oauth2/callback?state=correct&code=",
        "/oauth2/callback?state=correct&code=bad%0d%0aheader",
        "/oauth2/callback?state=correct",
        "/wrong?state=correct&code=secret",
        "http://evil.invalid/oauth2/callback?state=correct&code=secret",
        "//evil.invalid/oauth2/callback?state=correct&code=secret",
        "/oauth2/callback?state=correct&code=secret#fragment",
    ] {
        assert!(parse_callback(
            callback_request(target).as_bytes(),
            "127.0.0.1:12345",
            "correct"
        )
        .is_err());
    }
    let valid = callback_request("/oauth2/callback?state=correct&code=fictional-code");
    for invalid in [
        valid.replace("GET", "POST"),
        valid.replace("127.0.0.1:12345", "localhost:12345"),
        valid.replace("\r\n\r\n", "\r\nHost: 127.0.0.1:12345\r\n\r\n"),
        valid.replace("\r\n\r\n", "\r\nTransfer-Encoding: chunked\r\n\r\n"),
        format!("{valid}body"),
        "X".repeat(MAX_CALLBACK_BYTES + 1),
    ] {
        assert!(parse_callback(invalid.as_bytes(), "127.0.0.1:12345", "correct").is_err());
    }
    assert!(matches!(
        parse_callback(valid.as_bytes(), "127.0.0.1:12345", "correct"),
        Ok(Callback::Code(_))
    ));
    assert!(matches!(
        parse_callback(
            callback_request("/oauth2/callback?state=correct&error=access_denied").as_bytes(),
            "127.0.0.1:12345",
            "correct"
        ),
        Ok(Callback::Denied)
    ));
}

#[test]
fn pkce_matches_rfc7636_vector_and_requests_only_gmail_readonly() {
    let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    let url = authorization_url(
        "fictional.apps.googleusercontent.com",
        "http://127.0.0.1:12345/oauth2/callback",
        "random-state",
        verifier,
    )
    .unwrap();
    let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
    assert_eq!(
        query["code_challenge"],
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(query["scope"], GMAIL_READONLY_SCOPE);
    assert_eq!(query["access_type"], "offline");
    assert!(!query.contains_key("include_granted_scopes"));
    assert!(!url.as_str().contains(verifier));
    let first = random_secret().unwrap();
    let second = random_secret().unwrap();
    assert_eq!(first.len(), 43);
    assert_ne!(*first, *second);
}

#[tokio::test]
async fn real_loopback_flow_rejects_bad_state_then_exchanges_pkce_code() {
    let (mut service, store, mut browser) = test_service();
    service.configure(CLIENT_JSON).await.unwrap();
    let (endpoint, request) = token_server(200, format!(r#"{{"access_token":"fictional-access","refresh_token":"fictional-refresh","expires_in":3600,"token_type":"Bearer","scope":"{GMAIL_READONLY_SCOPE}"}}"#)).await;
    service.token_endpoint = Some(endpoint);
    let service = Arc::new(service);
    let connecting = tokio::spawn({
        let service = service.clone();
        async move { service.connect().await }
    });
    let url = browser.recv().await.unwrap();
    let rejected = browser_callback(&url, Some("incorrect-state")).await;
    assert!(rejected.starts_with("HTTP/1.1 400"));
    let accepted = browser_callback(&url, None).await;
    assert!(accepted.starts_with("HTTP/1.1 200"));
    assert!(!accepted.contains("fictional-auth-code"));
    assert!(accepted.contains("Cache-Control: no-store"));
    connecting.await.unwrap().unwrap();
    let request = request.await.unwrap();
    let (_, form) = request.split_once("\r\n\r\n").unwrap();
    let fields: std::collections::HashMap<_, _> = Url::parse(&format!("http://localhost/?{form}"))
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect();
    let authorization: std::collections::HashMap<_, _> = Url::parse(&url)
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect();
    assert_eq!(fields["code"], "fictional-auth-code");
    assert_eq!(fields["grant_type"], "authorization_code");
    assert_eq!(
        URL_SAFE_NO_PAD.encode(Sha256::digest(fields["code_verifier"].as_bytes())),
        authorization["code_challenge"]
    );
    assert_eq!(fields["redirect_uri"], authorization["redirect_uri"]);
    assert_eq!(fields["client_secret"], "fictional-client-secret");
    assert!(service.has_tokens().await.unwrap());
    assert_eq!(
        service.access_token(false).await.unwrap().expose(),
        b"fictional-access"
    );
    let stored = store.get(TOKEN_KEY).unwrap().unwrap();
    assert!(!std::str::from_utf8(stored.expose())
        .unwrap()
        .contains("fictional-access"));
}

#[tokio::test]
async fn cancellation_and_timeout_close_listener_without_saving_tokens() {
    let (service, _, mut browser) = test_service();
    service.configure(CLIENT_JSON).await.unwrap();
    let service = Arc::new(service);
    let connecting = tokio::spawn({
        let service = service.clone();
        async move { service.connect().await }
    });
    let url = browser.recv().await.unwrap();
    service.cancel();
    assert_eq!(connecting.await.unwrap(), Err(OAuthError::Cancelled));
    assert!(!service.has_tokens().await.unwrap());
    let query: std::collections::HashMap<_, _> = Url::parse(&url)
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect();
    let callback = Url::parse(&query["redirect_uri"]).unwrap();
    assert!(TcpStream::connect(("127.0.0.1", callback.port().unwrap()))
        .await
        .is_err());

    let (mut service, _, _browser) = test_service();
    service.configure(CLIENT_JSON).await.unwrap();
    service.connect_timeout = Duration::from_millis(20);
    assert_eq!(service.connect().await, Err(OAuthError::TimedOut));
    assert!(!service.has_tokens().await.unwrap());
}

#[tokio::test]
async fn refresh_preserves_old_refresh_token_when_response_omits_it() {
    let (mut service, _, _) = test_service();
    service.configure(CLIENT_JSON).await.unwrap();
    service
        .save_tokens(token_response("old-access", Some("old-refresh")), None)
        .await
        .unwrap();
    let (endpoint, request) = token_server(200, format!(r#"{{"access_token":"new-access","expires_in":3600,"token_type":"Bearer","scope":"{GMAIL_READONLY_SCOPE}"}}"#)).await;
    service.token_endpoint = Some(endpoint);
    assert_eq!(
        service.access_token(true).await.unwrap().expose(),
        b"new-access"
    );
    assert_eq!(
        service.load_refresh().unwrap().unwrap().refresh_token,
        "old-refresh"
    );
    assert!(request.await.unwrap().contains("refresh_token=old-refresh"));
    assert_eq!(
        service.access_token(false).await.unwrap().expose(),
        b"new-access"
    );
}

#[tokio::test]
async fn concurrent_expired_access_requests_share_one_refresh() {
    let (mut service, _, _) = test_service();
    service.configure(CLIENT_JSON).await.unwrap();
    service
        .save_tokens(token_response("old-access", Some("refresh")), None)
        .await
        .unwrap();
    service.access.lock().await.as_mut().unwrap().expires_at = now();
    let (endpoint, request) = token_server(
        200,
        r#"{"access_token":"new-access","expires_in":3600,"token_type":"Bearer"}"#.to_owned(),
    )
    .await;
    service.token_endpoint = Some(endpoint);
    let (first, second) = tokio::join!(service.access_token(false), service.access_token(false));
    assert_eq!(first.unwrap().expose(), b"new-access");
    assert_eq!(second.unwrap().expose(), b"new-access");
    request.await.unwrap();
}

#[tokio::test]
async fn revoked_grant_clears_tokens_and_errors_redact_provider_secrets() {
    let (mut service, _, _) = test_service();
    service.configure(CLIENT_JSON).await.unwrap();
    service
        .save_tokens(token_response("old-access", Some("old-refresh")), None)
        .await
        .unwrap();
    let (endpoint, request) = token_server(
        400,
        r#"{"error":"invalid_grant","error_description":"fictional-secret-must-not-leak"}"#
            .to_owned(),
    )
    .await;
    service.token_endpoint = Some(endpoint);
    let error = match service.access_token(true).await {
        Err(error) => error,
        Ok(_) => panic!("grant was accepted"),
    };
    assert!(error.requires_reconnect());
    assert!(!error.to_string().contains("fictional-secret"));
    assert!(!format!("{error:?}").contains("fictional-secret"));
    assert!(!service.has_tokens().await.unwrap());
    assert!(service.access.lock().await.is_none());
    assert!(service.configuration().await.unwrap().is_some());
    request.await.unwrap();
}

#[tokio::test]
async fn revoked_or_expired_refresh_and_corrupt_credentials_require_reconnect() {
    let (service, store, _) = test_service();
    service.configure(CLIENT_JSON).await.unwrap();
    service
        .save_tokens(token_response("access", Some("refresh")), None)
        .await
        .unwrap();
    store
        .set(
            TOKEN_KEY,
            &SecretValue::new(br#"{"refresh_token":"refresh","refresh_expires_at":1}"#.to_vec()),
        )
        .unwrap();
    assert_eq!(
        service.has_tokens().await,
        Err(OAuthError::ReconnectRequired)
    );
    assert!(matches!(
        service.access_token(false).await,
        Err(OAuthError::ReconnectRequired)
    ));
    assert!(!service.has_tokens().await.unwrap());
    store
        .set(
            TOKEN_KEY,
            &SecretValue::new(b"fictional-corrupt-secret".to_vec()),
        )
        .unwrap();
    assert_eq!(
        service.has_tokens().await,
        Err(OAuthError::InvalidCredentials)
    );
    service.disconnect().await.unwrap();
    assert!(!service.has_tokens().await.unwrap());
}

#[tokio::test]
async fn refuses_scope_escalation_invalid_tokens_and_missing_refresh_on_connect() {
    let (service, _, _) = test_service();
    for scope in [
        "",
        "https://mail.google.com/",
        "https://www.googleapis.com/auth/gmail.modify",
        "https://www.googleapis.com/auth/gmail.readonly openid",
    ] {
        let mut response = token_response("access", Some("refresh"));
        response.scope = Some(scope.to_owned());
        assert_eq!(
            service.save_tokens(response, None).await,
            Err(OAuthError::InvalidScope)
        );
        assert!(!service.has_tokens().await.unwrap());
    }
    let mut response = token_response("access", Some("refresh"));
    response.expires_in = 0;
    assert_eq!(
        service.save_tokens(response, None).await,
        Err(OAuthError::InvalidResponse)
    );
    assert_eq!(
        service
            .save_tokens(token_response("", Some("refresh")), None)
            .await,
        Err(OAuthError::InvalidResponse)
    );
    assert_eq!(
        service
            .save_tokens(token_response("access", None), None)
            .await,
        Err(OAuthError::ReconnectRequired)
    );
}

#[tokio::test]
async fn connected_setup_cannot_be_replaced_disconnect_retains_configuration() {
    let (service, _, _) = test_service();
    service.configure(CLIENT_JSON).await.unwrap();
    service
        .save_tokens(token_response("access", Some("refresh")), None)
        .await
        .unwrap();
    assert_eq!(
        service.configure(CLIENT_JSON).await,
        Err(OAuthError::AlreadyConnected)
    );
    assert_eq!(service.connect().await, Err(OAuthError::AlreadyConnected));
    service.disconnect().await.unwrap();
    assert!(!service.has_tokens().await.unwrap());
    assert!(service.configuration().await.unwrap().is_some());
    service.configure(CLIENT_JSON).await.unwrap();
}

#[tokio::test]
async fn token_http_responses_are_bounded_and_redirects_are_never_followed() {
    let (mut service, _, _) = test_service();
    let (endpoint, request) = token_server(200, "s".repeat(MAX_TOKEN_BYTES + 1)).await;
    service.token_endpoint = Some(endpoint);
    assert!(matches!(
        service.request_token(&[("code", "secret")]).await,
        Err(OAuthError::InvalidResponse)
    ));
    request.await.unwrap();
    let (endpoint, request) = token_server_headers(
        302,
        "fictional-sensitive-body".to_owned(),
        "Location: http://127.0.0.1:1/must-not-send-credentials\r\n",
    )
    .await;
    service.token_endpoint = Some(endpoint);
    assert!(matches!(
        service.request_token(&[("code", "secret")]).await,
        Err(OAuthError::AuthorizationFailed)
    ));
    request.await.unwrap();
}

#[tokio::test]
async fn cancellation_during_token_exchange_does_not_commit_credentials() {
    let (mut service, _, mut browser) = test_service();
    service.configure(CLIENT_JSON).await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    service.token_endpoint = Some(format!("http://{}/token", listener.local_addr().unwrap()));
    let service = Arc::new(service);
    let connecting = tokio::spawn({
        let service = service.clone();
        async move { service.connect().await }
    });
    let url = browser.recv().await.unwrap();
    browser_callback(&url, None).await;
    // Do not answer the token exchange. Cancellation must stop the HTTP wait.
    let (_exchange, _) = listener.accept().await.unwrap();
    service.cancel();
    let result = tokio::time::timeout(Duration::from_secs(2), connecting)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result, Err(OAuthError::Cancelled));
    assert!(!service.has_tokens().await.unwrap());
}

#[tokio::test]
async fn reconnect_can_replace_corrupt_or_expired_refresh_credentials() {
    for stored in [
        b"corrupt".as_slice(),
        br#"{"refresh_token":"refresh","refresh_expires_at":1}"#.as_slice(),
    ] {
        let (service, store, mut browser) = test_service();
        service.configure(CLIENT_JSON).await.unwrap();
        store
            .set(TOKEN_KEY, &SecretValue::new(stored.to_vec()))
            .unwrap();
        assert_eq!(
            service.configure(CLIENT_JSON).await,
            Err(OAuthError::AlreadyConnected)
        );
        let service = Arc::new(service);
        let connecting = tokio::spawn({
            let service = service.clone();
            async move { service.connect().await }
        });
        browser.recv().await.unwrap();
        service.cancel();
        assert_eq!(connecting.await.unwrap(), Err(OAuthError::Cancelled));
        assert!(!service.has_tokens().await.unwrap());
    }
}

#[tokio::test]
async fn refresh_rotation_and_refresh_lifetime_survive_storage_round_trip() {
    let (service, _, _) = test_service();
    let mut initial = token_response("access", Some("first-refresh"));
    initial.refresh_token_expires_in = Some(7200);
    service.save_tokens(initial, None).await.unwrap();
    let first = service.load_refresh().unwrap().unwrap();
    assert!(first.refresh_expires_at.unwrap() >= now() + 7100);
    service
        .save_tokens(
            token_response("second-access", Some("second-refresh")),
            Some(&first),
        )
        .await
        .unwrap();
    let second = service.load_refresh().unwrap().unwrap();
    assert_eq!(second.refresh_token, "second-refresh");
    assert_eq!(second.refresh_expires_at, first.refresh_expires_at);
}
