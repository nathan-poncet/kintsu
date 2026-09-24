//! One HTTP request per question: Ollama, anything OpenAI-compatible, or
//! the Anthropic Messages API. Blocking, non-streaming, with a timeout.

use std::time::Duration as StdDuration;

use serde_json::{Value, json};

use crate::entities::{ModelSpec, Provider};
use crate::use_cases::ports::{ModelError, ModelGateway, Prompt};

/// Wait this long when the model has no timeout of its own.
const DEFAULT_TIMEOUT_SECS: u64 = 45;

pub struct HttpModels;

/// A request, before it is sent: testable without a network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Value,
}

pub fn build_request(
    spec: &ModelSpec,
    key: Option<&str>,
    prompt: &Prompt,
) -> Result<Request, ModelError> {
    let base = spec
        .base_url
        .as_deref()
        .ok_or_else(|| ModelError::Unsupported(format!("{} has no base_url", spec.name)))?;
    let max_tokens = spec
        .max_output_tokens
        .map_or(prompt.max_tokens, |cap| cap.min(prompt.max_tokens).max(1))
        .max(1);
    let key_name =
        |what: &str| ModelError::MissingKey(format!("{} needs a key ({what})", spec.name));
    let messages = json!([
        {"role": "system", "content": prompt.system},
        {"role": "user", "content": prompt.user},
    ]);
    Ok(match spec.provider {
        Provider::Ollama => Request {
            url: format!("{base}/api/chat"),
            headers: vec![],
            body: json!({"model": spec.model, "messages": messages, "stream": false, "options": {"num_predict": max_tokens}}),
        },
        Provider::OpenAiCompatible => {
            let mut headers = vec![("content-type".to_string(), "application/json".to_string())];
            match key {
                Some(k) => headers.push(("authorization".to_string(), format!("Bearer {k}"))),
                None if spec.is_local() => {}
                None => return Err(key_name("bearer token")),
            }
            Request {
                url: format!("{base}/chat/completions"),
                headers,
                body: json!({"model": spec.model, "messages": messages, "max_tokens": max_tokens}),
            }
        }
        Provider::Anthropic => {
            let key = key.ok_or_else(|| key_name("x-api-key"))?;
            Request {
                url: format!("{base}/v1/messages"),
                headers: vec![
                    ("content-type".to_string(), "application/json".to_string()),
                    ("x-api-key".to_string(), key.to_string()),
                    ("anthropic-version".to_string(), "2023-06-01".to_string()),
                ],
                body: json!({
                    "model": spec.model,
                    "system": prompt.system,
                    "max_tokens": max_tokens,
                    "messages": [{"role": "user", "content": prompt.user}],
                }),
            }
        }
        Provider::CliAgent => {
            return Err(ModelError::Unsupported(format!(
                "{} is a CLI agent, not a model endpoint",
                spec.name
            )));
        }
    })
}

pub fn parse_answer(provider: Provider, status: u16, body: &Value) -> Result<String, ModelError> {
    if !(200..300).contains(&status) {
        let detail = body
            .pointer("/error/message")
            .or_else(|| body.get("error"))
            .map(|v| {
                v.as_str()
                    .map(String::from)
                    .unwrap_or_else(|| v.to_string())
            })
            .unwrap_or_default();
        return Err(ModelError::Refused(
            format!("HTTP {status} {}", detail.trim())
                .trim()
                .to_string(),
        ));
    }
    let text = match provider {
        Provider::Ollama => body.pointer("/message/content").and_then(Value::as_str),
        Provider::OpenAiCompatible => body
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str),
        Provider::Anthropic => body
            .get("content")
            .and_then(Value::as_array)
            .and_then(|parts| {
                parts
                    .iter()
                    .find_map(|p| p.get("text").and_then(Value::as_str))
            }),
        Provider::CliAgent => None,
    };
    text.map(|t| t.trim().to_string())
        .ok_or_else(|| ModelError::Malformed("no text in the answer".into()))
}

impl ModelGateway for HttpModels {
    fn complete(
        &self,
        spec: &ModelSpec,
        key: Option<&str>,
        prompt: &Prompt,
    ) -> Result<String, ModelError> {
        let request = build_request(spec, key, prompt)?;
        let timeout = spec
            .timeout
            .map_or(StdDuration::from_secs(DEFAULT_TIMEOUT_SECS), |d| {
                StdDuration::from_millis(d.as_millis())
            });
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .build();
        let agent: ureq::Agent = config.into();
        let mut call = agent.post(&request.url);
        for (name, value) in &request.headers {
            call = call.header(name.as_str(), value.as_str());
        }
        let mut response = call
            .send_json(&request.body)
            .map_err(|e| ModelError::Unreachable(e.to_string()))?;
        let status = response.status().as_u16();
        let body: Value = response.body_mut().read_json().unwrap_or(Value::Null);
        parse_answer(spec.provider, status, &body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{KeySource, Tier};

    fn spec(provider: Provider, base_url: &str) -> ModelSpec {
        ModelSpec {
            name: "m".into(),
            provider,
            model: "model-id".into(),
            base_url: Some(base_url.into()),
            key: KeySource::None,
            tier: Tier::Small,
            timeout: None,
            max_output_tokens: Some(100),
        }
    }

    fn prompt() -> Prompt {
        Prompt {
            system: "sys".into(),
            user: "usr".into(),
            max_tokens: 400,
        }
    }

    #[test]
    fn each_provider_gets_its_own_shape_and_the_smaller_token_cap() {
        let ollama = build_request(
            &spec(Provider::Ollama, "http://127.0.0.1:11434"),
            None,
            &prompt(),
        )
        .unwrap();
        assert_eq!(ollama.url, "http://127.0.0.1:11434/api/chat");
        assert_eq!(ollama.body["stream"], false);
        assert_eq!(ollama.body["options"]["num_predict"], 100);

        let openai = build_request(
            &spec(Provider::OpenAiCompatible, "https://api.openai.com/v1"),
            Some("k"),
            &prompt(),
        )
        .unwrap();
        assert_eq!(openai.url, "https://api.openai.com/v1/chat/completions");
        assert!(
            openai
                .headers
                .contains(&("authorization".into(), "Bearer k".into()))
        );
        assert_eq!(openai.body["messages"][0]["role"], "system");

        let anthropic = build_request(
            &spec(Provider::Anthropic, "https://api.anthropic.com"),
            Some("k"),
            &prompt(),
        )
        .unwrap();
        assert_eq!(anthropic.url, "https://api.anthropic.com/v1/messages");
        assert!(
            anthropic
                .headers
                .contains(&("x-api-key".into(), "k".into()))
        );
        assert_eq!(anthropic.body["system"], "sys");
        assert_eq!(anthropic.body["max_tokens"], 100);
    }

    #[test]
    fn remote_endpoints_need_a_key_local_ones_do_not() {
        assert!(matches!(
            build_request(
                &spec(Provider::Anthropic, "https://api.anthropic.com"),
                None,
                &prompt()
            ),
            Err(ModelError::MissingKey(_))
        ));
        assert!(matches!(
            build_request(
                &spec(Provider::OpenAiCompatible, "https://openrouter.ai/api/v1"),
                None,
                &prompt()
            ),
            Err(ModelError::MissingKey(_))
        ));
        assert!(
            build_request(
                &spec(Provider::OpenAiCompatible, "http://localhost:1234/v1"),
                None,
                &prompt()
            )
            .is_ok()
        );
        assert!(matches!(
            build_request(&spec(Provider::CliAgent, "x"), None, &prompt()),
            Err(ModelError::Unsupported(_))
        ));
    }

    #[test]
    fn answers_are_read_from_each_providers_place() {
        assert_eq!(
            parse_answer(
                Provider::Ollama,
                200,
                &json!({"message": {"content": " hi "}})
            )
            .unwrap(),
            "hi"
        );
        assert_eq!(
            parse_answer(
                Provider::OpenAiCompatible,
                200,
                &json!({"choices": [{"message": {"content": "hi"}}]})
            )
            .unwrap(),
            "hi"
        );
        assert_eq!(
            parse_answer(
                Provider::Anthropic,
                200,
                &json!({"content": [{"type": "text", "text": "hi"}]})
            )
            .unwrap(),
            "hi"
        );
        assert_eq!(
            parse_answer(Provider::Ollama, 200, &json!({"nope": 1})).unwrap_err(),
            ModelError::Malformed("no text in the answer".into())
        );
        assert_eq!(
            parse_answer(
                Provider::Anthropic,
                401,
                &json!({"error": {"message": "invalid x-api-key"}})
            )
            .unwrap_err(),
            ModelError::Refused("HTTP 401 invalid x-api-key".into())
        );
        assert_eq!(
            parse_answer(Provider::Ollama, 500, &Value::Null).unwrap_err(),
            ModelError::Refused("HTTP 500".into())
        );
    }

    #[test]
    fn an_unreachable_endpoint_is_reported_not_panicked() {
        let mut s = spec(Provider::Ollama, "http://127.0.0.1:9");
        s.timeout = Some(crate::entities::Duration::from_millis(500));
        assert!(matches!(
            HttpModels.complete(&s, None, &prompt()),
            Err(ModelError::Unreachable(_))
        ));
    }
}
