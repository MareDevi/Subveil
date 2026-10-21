//! Sanitized cloud failures and bounded recovery shared by speech transports.
use std::{fmt, time::Duration};
use tokio_tungstenite::tungstenite::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    Authentication,
    Configuration,
    RateLimited(Option<u64>),
    Temporary,
}
impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Authentication => "云服务拒绝认证或权限；请修改凭据后重新开始",
            Self::Configuration => "云服务配置无效；请检查端点、模型与部署后重新开始",
            Self::RateLimited(_) => "云服务限流；正在按限制等待",
            Self::Temporary => "云服务暂时不可用；请检查网络与服务状态",
        })
    }
}
impl std::error::Error for Failure {}

pub(crate) fn handshake(error: Error) -> anyhow::Error {
    match error {
        Error::Http(response) => {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok());
            status(response.status().as_u16(), retry_after).into()
        }
        _ => Failure::Temporary.into(),
    }
}
fn status(code: u16, retry_after: Option<u64>) -> Failure {
    match code {
        401 | 403 => Failure::Authentication,
        400 | 404 | 405 | 422 => Failure::Configuration,
        429 => Failure::RateLimited(retry_after),
        _ => Failure::Temporary,
    }
}
// Never retain or display arbitrary vendor messages, URLs, bodies or key values.
pub(crate) fn service(value: &serde_json::Value) -> Failure {
    let code = value
        .pointer("/error/code")
        .or_else(|| value.pointer("/header/error_code"))
        .or_else(|| value.get("code"))
        .or_else(|| value.get("error_code"));
    if let Some(code) = code.and_then(serde_json::Value::as_u64) {
        return status(code.min(u64::from(u16::MAX)) as u16, None);
    }
    match code.and_then(serde_json::Value::as_str).unwrap_or("") {
        "invalid_api_key"
        | "authentication_error"
        | "unauthorized"
        | "permission_denied"
        | "UNAUTHENTICATED"
        | "PERMISSION_DENIED"
        | "InvalidApiKey" => Failure::Authentication,
        "invalid_request_error"
        | "invalid_argument"
        | "model_not_found"
        | "INVALID_ARGUMENT"
        | "InvalidParameter" => Failure::Configuration,
        "rate_limit_exceeded" | "resource_exhausted" | "RESOURCE_EXHAUSTED" => {
            Failure::RateLimited(None)
        }
        _ => Failure::Temporary,
    }
}

#[derive(Default)]
pub(crate) struct Recovery {
    attempts: u32,
}
impl Recovery {
    pub fn next(&mut self, error: &anyhow::Error, healthy: Duration) -> Option<Duration> {
        let failure = error
            .chain()
            .find_map(|e| e.downcast_ref::<Failure>())
            .copied();
        if matches!(
            failure,
            Some(Failure::Authentication | Failure::Configuration)
        ) {
            return None;
        }
        if healthy >= Duration::from_secs(30) {
            self.attempts = 0;
        }
        if self.attempts >= 4 {
            return None;
        }
        let delay = 1_u64 << self.attempts;
        self.attempts += 1;
        // Do not retry earlier than Retry-After. Very long limits require the
        // user to start again instead of parking an authorized upload forever.
        let requested = match failure {
            Some(Failure::RateLimited(Some(s))) => s,
            _ => 0,
        };
        if requested > 60 {
            return None;
        }
        Some(Duration::from_secs(delay.max(requested)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn permanent_failures_never_retry_and_messages_are_sanitized() {
        for code in [400, 401, 403, 404, 422] {
            let error: anyhow::Error = status(code, None).into();
            assert!(Recovery::default().next(&error, Duration::ZERO).is_none());
        }
        let failure = service(
            &serde_json::json!({"error":{"code":"invalid_api_key","message":"secret text"}}),
        );
        assert_eq!(failure, Failure::Authentication);
        assert!(!failure.to_string().contains("secret"));
    }
    #[test]
    fn recovery_is_bounded_respects_rate_limit_and_resets_after_healthy_stream() {
        let mut recovery = Recovery::default();
        let error: anyhow::Error = Failure::Temporary.into();
        for seconds in [1, 2, 4, 8] {
            assert_eq!(
                recovery.next(&error, Duration::ZERO),
                Some(Duration::from_secs(seconds))
            );
        }
        assert!(recovery.next(&error, Duration::ZERO).is_none());
        assert_eq!(
            recovery.next(&error, Duration::from_secs(30)),
            Some(Duration::from_secs(1))
        );
        let limited: anyhow::Error = Failure::RateLimited(Some(20)).into();
        assert_eq!(
            recovery.next(&limited, Duration::ZERO),
            Some(Duration::from_secs(20))
        );
        assert!(
            recovery
                .next(&Failure::RateLimited(Some(120)).into(), Duration::ZERO)
                .is_none()
        );
    }
}
