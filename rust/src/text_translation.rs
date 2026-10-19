//! LecSync text translation: documented request keys, polling, and idempotent retries.
use super::*;

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct Job {
    pub text: String,
    pub source_language: String,
    pub target_language: String,
    pub key: String,
    pub request_id: Option<String>,
    pub revision: u64,
    pub succeeded: bool,
    pub terminal_failure: bool,
}

pub(super) struct Failure {
    pub message: String,
    pub terminal: bool,
}

impl Failure {
    fn new(message: &str, terminal: bool) -> Self {
        Self {
            message: message.into(),
            terminal,
        }
    }
}

pub(super) fn new_key() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("lingodesk-text-{time}-{n}")
}

pub(super) fn translate(
    api_key: &str,
    job: &Job,
    remember: impl FnMut(&str) -> Result<(), String>,
) -> Result<String, Failure> {
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| Failure::new("无法初始化网络", false))?;
    execute(
        &client,
        "https://api.lecsync.com/v1",
        api_key,
        job,
        remember,
        Duration::from_secs(45),
        Duration::from_millis(700),
    )
}

fn status_error(status: u16, body: &Value) -> Failure {
    let code = body["error"].as_str().unwrap_or("");
    let message = match (status, code) {
        (401, _) => "API Key 无效或已过期",
        (402, _) => "API 钱包余额不足",
        (403, _) | (_, "ADMISSION_DISABLED") => "API Key 没有权限",
        (_, "UNSUPPORTED_LANGUAGE" | "LANGUAGE_UNDETERMINED" | "CAPABILITY_UNAVAILABLE") => {
            "不支持的语言"
        }
        (429, _) => "请求过于频繁或会话数已达上限",
        (503, _) => "LecSync 当前没有可用节点",
        (504, _) => "翻译仍在处理中，请稍后再次保存。",
        (300..=399, _) => "拒绝接口重定向以保护密钥",
        _ => "操作失败",
    };
    Failure::new(
        message,
        matches!(status, 400 | 401 | 402 | 403 | 404 | 409 | 410 | 422),
    )
}

fn valid_request_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn execute(
    client: &reqwest::blocking::Client,
    base: &str,
    api_key: &str,
    job: &Job,
    mut remember: impl FnMut(&str) -> Result<(), String>,
    budget: Duration,
    interval: Duration,
) -> Result<String, Failure> {
    if api_key.trim().is_empty() {
        return Err(Failure::new("请先保存 LecSync API Key", true));
    }
    let deadline = Instant::now() + budget;
    let mut request_id = job.request_id.clone().filter(|id| valid_request_id(id));
    let mut submit = request_id.is_none();
    loop {
        let response = if submit {
            client.post(format!("{base}/translate"))
                .bearer_auth(api_key.trim()).header("Idempotency-Key", &job.key)
                .json(&json!({"text":job.text,"source_lang":job.source_language,"target_lang":job.target_language}))
                .send()
        } else {
            let id = request_id.as_deref().ok_or_else(|| Failure::new("会话响应格式错误", false))?;
            client.get(format!("{base}/requests/{id}"))
                .bearer_auth(api_key.trim()).send()
        }.map_err(|_| Failure::new("连接 LecSync 失败，请检查网络", false))?;
        let status = response.status().as_u16();
        let body: Value = response.json().unwrap_or(Value::Null);
        if let Some(id) = body["request_id"]
            .as_str()
            .filter(|id| valid_request_id(id))
        {
            // Only an accepted execution has a retrievable request record. Error
            // responses also have diagnostic request IDs, which are not jobs.
            if (200..300).contains(&status) {
                remember(id).map_err(|s| Failure::new(&s, false))?;
                request_id = Some(id.into());
            }
        }
        if !(200..300).contains(&status) {
            return Err(status_error(status, &body));
        }
        let result = body
            .get("result")
            .filter(|v| v.is_object())
            .unwrap_or(&body);
        let stage = body["status"]
            .as_str()
            .or_else(|| result["status"].as_str())
            .unwrap_or("");
        match stage {
            "failed" | "cancelled" => return Err(Failure::new("操作失败", true)),
            "accepted" | "processing" => {}
            _ => {
                if let Some(text) = result["text"].as_str().filter(|s| !s.trim().is_empty()) {
                    if result["target_lang"]
                        .as_str()
                        .is_some_and(|lang| lang != job.target_language)
                    {
                        return Err(Failure::new("会话响应格式错误", true));
                    }
                    return Ok(text.into());
                }
                return Err(Failure::new("会话响应格式错误", false));
            }
        }
        if request_id.is_none() {
            return Err(Failure::new("会话响应格式错误", false));
        }
        if Instant::now() >= deadline {
            return Err(Failure::new("翻译仍在处理中，请稍后再次保存。", false));
        }
        submit = false;
        thread::sleep(interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read};
    use std::net::TcpListener;

    // Exercise the actual HTTP client against a local server, including lost
    // responses. No service key or billable remote request is used by tests.
    fn server(replies: Vec<Option<(u16, Value)>>) -> (String, thread::JoinHandle<Vec<Value>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let handle = thread::spawn(move || {
            let mut requests = Vec::new();
            for reply in replies {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            thread::sleep(Duration::from_millis(5))
                        }
                        Err(e) => panic!("missing HTTP request: {e}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                let mut headers = BTreeMap::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line.trim().is_empty() {
                        break;
                    }
                    let (name, value) = line.split_once(':').unwrap();
                    headers.insert(name.to_ascii_lowercase(), value.trim().to_owned());
                }
                let len = headers
                    .get("content-length")
                    .map(|s| s.parse().unwrap())
                    .unwrap_or(0);
                let mut bytes = vec![0; len];
                reader.read_exact(&mut bytes).unwrap();
                requests.push(json!({"line":first.trim(), "headers":headers, "body":serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null)}));
                if let Some((status, body)) = reply {
                    let data = body.to_string();
                    write!(stream, "HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nLocation: http://127.0.0.1:9/untrusted\r\n\r\n{data}",data.len()).unwrap();
                }
            }
            requests
        });
        (base, handle)
    }

    fn job() -> Job {
        Job {
            text: "Edited sentence 😀".into(),
            source_language: "en".into(),
            target_language: "zh".into(),
            key: new_key(),
            ..Job::default()
        }
    }
    fn call(
        base: &str,
        job: &Job,
        remember: impl FnMut(&str) -> Result<(), String>,
        budget: Duration,
    ) -> Result<String, Failure> {
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        execute(
            &client,
            base,
            "fake-test-key",
            job,
            remember,
            budget,
            Duration::from_millis(1),
        )
    }

    #[test]
    fn documented_post_and_poll_preserve_input_authentication_and_request_identity() {
        let (base, server) = server(vec![
            Some((
                202,
                json!({"request_id":"req_translation","status":"accepted"}),
            )),
            Some((
                200,
                json!({"request_id":"req_translation","status":"succeeded","result":{"text":"修改后的句子 😀","target_lang":"zh"}}),
            )),
        ]);
        let job = job();
        let mut ids = Vec::new();
        let text = call(
            &base,
            &job,
            |id| {
                ids.push(id.to_owned());
                Ok(())
            },
            Duration::from_secs(1),
        )
        .ok()
        .unwrap();
        assert_eq!(text, "修改后的句子 😀");
        assert_eq!(ids, vec!["req_translation", "req_translation"]);
        let requests = server.join().unwrap();
        assert_eq!(requests[0]["line"], "POST /translate HTTP/1.1");
        assert_eq!(
            requests[0]["headers"]["authorization"],
            "Bearer fake-test-key"
        );
        assert_eq!(requests[0]["headers"]["idempotency-key"], job.key);
        assert_eq!(
            requests[0]["body"],
            json!({"text":job.text,"source_lang":"en","target_lang":"zh"})
        );
        assert_eq!(
            requests[1]["line"],
            "GET /requests/req_translation HTTP/1.1"
        );
        assert!(requests[1]["body"].is_null());
    }

    #[test]
    fn lost_post_response_retries_identical_idempotent_request_and_accepts_stored_result() {
        let (base, server) = server(vec![
            None,
            Some((
                200,
                json!({"status":"succeeded","result":{"text":"译文","target_lang":"zh"}}),
            )),
        ]);
        let job = job();
        let failure = call(&base, &job, |_| Ok(()), Duration::from_secs(1))
            .err()
            .unwrap();
        assert!(!failure.terminal);
        assert_eq!(
            call(&base, &job, |_| Ok(()), Duration::from_secs(1))
                .ok()
                .unwrap(),
            "译文"
        );
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0], requests[1]);
    }

    #[test]
    fn resume_queries_existing_request_and_processing_timeout_remains_retryable() {
        let (base, server) = server(vec![Some((
            200,
            json!({"request_id":"req_existing","status":"processing"}),
        ))]);
        let mut job = job();
        job.request_id = Some("req_existing".into());
        let mut remembered = String::new();
        let failure = call(
            &base,
            &job,
            |id| {
                remembered = id.into();
                Ok(())
            },
            Duration::ZERO,
        )
        .err()
        .unwrap();
        assert!(!failure.terminal);
        assert_eq!(remembered, "req_existing");
        assert_eq!(
            server.join().unwrap()[0]["line"],
            "GET /requests/req_existing HTTP/1.1"
        );
    }

    #[test]
    fn errors_do_not_remember_diagnostic_ids_and_redirects_never_forward_the_key() {
        for status in [302, 401, 402, 429, 503, 504] {
            let (base, server) = server(vec![Some((
                status,
                json!({"request_id":"req_diagnostic","error":"example"}),
            ))]);
            let failure = call(
                &base,
                &job(),
                |_| panic!("diagnostic ID is not a resumable translation"),
                Duration::ZERO,
            )
            .err()
            .unwrap();
            if status == 402 {
                assert_eq!(failure.message, "API 钱包余额不足");
                assert!(failure.terminal);
            }
            if status == 302 {
                assert_eq!(failure.message, "拒绝接口重定向以保护密钥");
            }
            assert_eq!(server.join().unwrap().len(), 1);
        }
    }

    #[test]
    fn mismatched_language_and_failed_jobs_do_not_produce_a_translation() {
        for body in [
            json!({"status":"succeeded","text":"wrong","target_lang":"en"}),
            json!({"status":"failed","text":"partial"}),
        ] {
            let (base, server) = server(vec![Some((200, body))]);
            assert!(
                call(&base, &job(), |_| Ok(()), Duration::ZERO)
                    .err()
                    .unwrap()
                    .terminal
            );
            server.join().unwrap();
        }
    }
}
