//! Opt-in HTTPS transport implementing the same BandoGateway boundary.
//! The server contract is documented in egui-app/API-CONTRACT.md.
//! No server is currently deployed by this repository. This adapter is NOT
//! enabled unless BANDO_HUB_GATEWAY=http is explicitly configured.

use crate::gateway::{BandoGateway, ReadOperation};
use reqwest::{blocking::Client, Url};
use serde_json::{json, Value};
use std::time::Duration;

pub struct HttpGateway {
    client: Client,
    base_url: Url,
    access_token: String,
}

impl HttpGateway {
    pub fn from_environment() -> Result<Self, String> {
        let url = std::env::var("BANDO_HUB_API_BASE_URL")
            .map_err(|_| "서버 사용 시 BANDO_HUB_API_BASE_URL 설정이 필요합니다.".to_owned())?;
        let base_url = Url::parse(&url)
            .map_err(|_| "서버 API 주소 형식이 잘못되었습니다.".to_owned())?;
        let loopback = matches!(base_url.host_str(),
            Some("localhost") | Some("127.0.0.1") | Some("[::1]"));
        if base_url.scheme() != "https" && !(loopback && base_url.scheme() == "http") {
            return Err("서버 API는 HTTPS여야 합니다. 개발용 localhost만 HTTP가 허용됩니다.".into());
        }
        if !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
        {
            return Err("서버 API 주소에 사용자정보·쿼리·토큰을 포함하지 마세요.".into());
        }
        let access_token = std::env::var("BANDO_HUB_API_TOKEN")
            .map_err(|_| "서버 접속 토큰 BANDO_HUB_API_TOKEN 설정이 필요합니다.".to_owned())?;
        if access_token.trim().is_empty() {
            return Err("서버 접속 토큰이 비어 있습니다.".into());
        }
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(25))
            .build()
            .map_err(|_| "HTTPS 클라이언트를 초기화하지 못했습니다.".to_owned())?;
        Ok(Self { client, base_url, access_token })
    }

    fn endpoint(&self, suffix: &str) -> Url {
        let mut url = self.base_url.clone();
        let prefix = self.base_url.path().trim_end_matches('/');
        url.set_path(&format!("{prefix}/api/v1/{suffix}"));
        url
    }

    fn decode(response: reqwest::blocking::Response) -> Result<Value, String> {
        let status = response.status();
        if !status.is_success() {
            // Never echo untrusted server bodies, credentials, or raw order
            // text into logs or on-screen error messages.
            return Err(format!("서버 API 요청 실패 (HTTP {})", status.as_u16()));
        }
        response.json::<Value>()
            .map_err(|_| "서버가 올바른 JSON 응답을 반환하지 않았습니다.".to_owned())
    }
}

impl BandoGateway for HttpGateway {
    fn label(&self) -> &'static str {
        "HTTPS 서버"
    }

    fn load_state(&self) -> Result<Value, String> {
        let response = self.client.get(self.endpoint("state"))
            .bearer_auth(&self.access_token)
            .send()
            .map_err(|_| "서버 상태 조회에 실패했습니다. 연결과 인증을 확인하세요.".to_owned())?;
        let state = Self::decode(response)?;
        if !state.get("products").is_some_and(Value::is_array)
            || !state.get("aliases").is_some_and(Value::is_array)
        {
            return Err("서버 상태에 products/aliases 배열이 없습니다.".into());
        }
        Ok(state)
    }

    fn execute(&self, operation: ReadOperation, payload: Value) -> Result<Value, String> {
        let body = json!({
            "domain": operation.domain(),
            "operation": operation.operation(),
            "payload": payload,
        });
        let response = self.client.post(self.endpoint("business-core"))
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .map_err(|_| "서버 업무 API 요청에 실패했습니다. 연결 상태를 확인하세요.".to_owned())?;
        let json = Self::decode(response)?;
        json.get("result").cloned()
            .ok_or_else(|| "서버 업무 API 응답에 result 필드가 없습니다.".to_owned())
    }
}
