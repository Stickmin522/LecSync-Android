mod caption_edits;
mod captions;
mod languages;
mod record_edits;
mod text_translation;
mod transport;
use jni::{
    JNIEnv,
    objects::{JObject, JShortArray, JString},
    sys::{jint, jstring},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{BufWriter, Seek, SeekFrom, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    sync::{
        Arc, LazyLock, Mutex,
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tungstenite::{Message, WebSocket, client::IntoClientRequest, stream::MaybeTlsStream};

const RATE: u64 = 16000;
type Socket = WebSocket<MaybeTlsStream<TcpStream>>;
#[derive(Clone, Serialize, Deserialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub id: String,
    pub text: String,
    pub translation: String,
    pub start_ms: Option<u64>,
    pub end_ms: Option<u64>,
    pub speaker: String,
    pub language: String,
    pub enhanced: bool,
}
#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct View {
    id: String,
    title: String,
    created_at: u64,
    source: String,
    pair: String,
    phase: String,
    duration_ms: u64,
    level: f64,
    segments: Vec<Segment>,
    captions: Vec<captions::Caption>,
    caption_edits: caption_edits::Edits,
    next_caption_id: u64,
    structured_preview: bool,
    connection: String,
    chinese_sections: Vec<Value>,
    chinese_digest_source: Vec<Value>,
    digest_translated: bool,
    preview: String,
    preview_translation: String,
    sections: Vec<Value>,
    usage: Value,
    error: String,
    warning: String,
    audio_path: String,
    dropped_frames: u64,
}
impl View {
    fn idle() -> Self {
        Self {
            phase: "idle".into(),
            usage: json!({}),
            ..Self::default()
        }
    }
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    api_key: String,
    #[serde(default = "default_pair")]
    pair: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    title: String,
    #[serde(default = "yes")]
    speakers: bool,
    #[serde(default = "yes")]
    digest: bool,
    #[serde(default)]
    enhance: bool,
    directory: String,
}
fn default_pair() -> String {
    "ja-zh".into()
}
fn yes() -> bool {
    true
}
struct Wave {
    file: BufWriter<File>,
    samples: u64,
    path: PathBuf,
}
impl Wave {
    fn new(path: PathBuf) -> Result<Self, String> {
        let mut x = Self {
            file: BufWriter::new(File::create(&path).map_err(|_| "无法创建录音文件")?),
            samples: 0,
            path,
        };
        x.header()?;
        Ok(x)
    }
    fn header(&mut self) -> Result<(), String> {
        let len = u32::try_from(self.samples * 2)
            .map_err(|_| "录音达到 WAV 文件容量限制，请结束并开始新录音")?;
        let end = self
            .file
            .stream_position()
            .map_err(|_| "读取录音位置失败")?;
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|_| "写入录音失败")?;
        let mut h = Vec::with_capacity(44);
        h.extend(b"RIFF");
        h.extend((36u32 + len).to_le_bytes());
        h.extend(b"WAVEfmt ");
        h.extend(16u32.to_le_bytes());
        h.extend(1u16.to_le_bytes());
        h.extend(1u16.to_le_bytes());
        h.extend(16000u32.to_le_bytes());
        h.extend(32000u32.to_le_bytes());
        h.extend(2u16.to_le_bytes());
        h.extend(16u16.to_le_bytes());
        h.extend(b"data");
        h.extend(len.to_le_bytes());
        self.file.write_all(&h).map_err(|_| "写入 WAV 头失败")?;
        self.file.flush().map_err(|_| "保存录音失败")?;
        self.file
            .seek(SeekFrom::Start(end.max(44)))
            .map_err(|_| "定位录音失败")?;
        Ok(())
    }
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        if self.samples * 2 + bytes.len() as u64 > u32::MAX as u64 - 36 {
            return Err("录音达到 WAV 容量限制".into());
        }
        self.file
            .write_all(bytes)
            .map_err(|_| "空间不足，录音写入失败")?;
        self.samples += bytes.len() as u64 / 2;
        if self.samples % RATE < bytes.len() as u64 / 2 {
            self.header()?;
        }
        Ok(())
    }
}
enum Control {
    Pause,
    Resume,
    Stop,
}
struct Frame {
    bytes: Vec<u8>,
    start_ms: u64,
}
struct Engine {
    control: Sender<Control>,
    frames: SyncSender<Frame>,
    view: Arc<Mutex<View>>,
    wave: Arc<Mutex<Wave>>,
}
static ENGINE: LazyLock<Mutex<Option<Engine>>> = LazyLock::new(|| Mutex::new(None));
static LAST: LazyLock<Mutex<View>> = LazyLock::new(|| Mutex::new(View::idle()));
fn mutate(view: &Arc<Mutex<View>>, f: impl FnOnce(&mut View)) {
    if let Ok(mut v) = view.lock() {
        f(&mut v)
    }
}
fn phase(view: &Arc<Mutex<View>>, p: &str) {
    mutate(view, |v| {
        v.phase = p.into();
        v.level = 0.0;
    });
}
fn snapshot() -> View {
    if let Ok(e) = ENGINE.lock() {
        if let Some(e) = e.as_ref() {
            if let Ok(v) = e.view.lock() {
                return caption_edits::render(&v);
            }
        }
    }
    LAST.lock()
        .map(|x| caption_edits::render(&x))
        .unwrap_or_else(|_| View::idle())
}
fn error_for_status(status: u16) -> String {
    match status {
        400 => "LecSync 拒绝了会话参数",
        401 => "API Key 无效或已过期",
        402 => "API 钱包余额不足",
        403 => "API Key 没有权限",
        429 => "请求过于频繁或会话数已达上限",
        503 => "LecSync 当前没有可用节点",
        300..=399 => "拒绝接口重定向以保护密钥",
        _ => "LecSync 创建会话失败",
    }
    .into()
}
fn open(c: &Config) -> Result<Socket, String> {
    #[cfg(feature = "test-harness")]
    if c.api_key == "LOCAL_TEST_ONLY" {
        let tcp = TcpStream::connect_timeout(
            &"127.0.0.1:8787".parse().map_err(|_| "测试地址错误")?,
            Duration::from_secs(3),
        )
        .map_err(|_| "测试节点未启动")?;
        tcp.set_read_timeout(Some(Duration::from_secs(3)))
            .map_err(|_| "测试超时设置失败")?;
        tcp.set_write_timeout(Some(Duration::from_secs(3)))
            .map_err(|_| "测试超时设置失败")?;
        let (mut socket, _) =
            tungstenite::client_tls("ws://127.0.0.1:8787", tcp).map_err(|_| "测试节点握手失败")?;
        let _ = socket.read().map_err(|_| "测试节点未就绪")?;
        set_nonblocking(&mut socket)?;
        return Ok(socket);
    }
    if c.api_key.trim().is_empty() {
        return Err("请先填写 LecSync API Key".into());
    }
    let languages = languages::parse_pair(&c.pair)?;
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| "无法初始化网络")?;
    let response=client.post("https://api.lecsync.com/v1/realtime/connect").bearer_auth(c.api_key.trim()).json(&json!({"audio":{"encoding":"pcm_s16le","sampleRate":16000,"channels":1},"transcribe":{"languages":languages,"strict":false,"identifyLanguage":true,"displayMode":"classic","segmentation":"complete","speakers":c.speakers},"translate":{"languages":languages},"digest":c.digest,"aiEnhance":c.enhance,"store":false})).send().map_err(|_|"连接 LecSync 失败，请检查网络")?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(error_for_status(status));
    }
    let response: Value = response.json().map_err(|_| "会话响应格式错误")?;
    let mut url = url::Url::parse(response["url"].as_str().ok_or("缺少会话地址")?)
        .map_err(|_| "无效会话地址")?;
    let host = url.host_str().unwrap_or("");
    if url.scheme() != "wss"
        || !(host == "lecsync.com" || host.ends_with(".lecsync.com"))
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("拒绝不可信的会话地址".into());
    }
    if !url.path().starts_with("/v1/sessions/") {
        let id = response["sessionId"].as_str().ok_or("缺少会话 ID")?;
        if !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err("无效会话 ID".into());
        }
        url.set_path(&format!("/v1/sessions/{id}"));
    }
    let token = response["token"].as_str().ok_or("缺少会话令牌")?;
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|_| "无法建立实时连接")?;
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        format!("lecsync.realtime.v1, {token}")
            .parse()
            .map_err(|_| "无效会话令牌")?,
    );
    // Explicit timed TCP connection; tungstenite's convenience connect has no timeout.
    use std::net::ToSocketAddrs;
    let addresses = (host_owned(&url), url.port_or_known_default().unwrap_or(443))
        .to_socket_addrs()
        .map_err(|_| "无法解析实时节点")?;
    let mut tcp = None;
    for address in addresses {
        if let Ok(s) = TcpStream::connect_timeout(&address, Duration::from_secs(8)) {
            tcp = Some(s);
            break;
        }
    }
    let tcp = tcp.ok_or("连接实时节点超时")?;
    tcp.set_read_timeout(Some(Duration::from_secs(15)))
        .map_err(|_| "设置连接超时失败")?;
    tcp.set_write_timeout(Some(Duration::from_secs(15)))
        .map_err(|_| "设置连接超时失败")?;
    let config = tungstenite::protocol::WebSocketConfig::default()
        .write_buffer_size(0)
        .max_write_buffer_size(64000)
        .max_message_size(Some(4 * 1024 * 1024));
    let (mut socket, _) = tungstenite::client_tls_with_config(request, tcp, Some(config), None)
        .map_err(|_| "实时节点握手失败")?;
    set_nonblocking(&mut socket)?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match socket.read() {
            Ok(Message::Text(t)) => {
                let msg: Value = serde_json::from_str(&t).unwrap_or(Value::Null);
                if msg["type"] == "session.ready" {
                    return Ok(socket);
                }
                if msg["type"] == "session.error" {
                    return Err("实时节点拒绝了会话，请检查语言配置及权限".into());
                }
            }
            Ok(Message::Close(_)) => return Err("实时节点提前断开".into()),
            Err(tungstenite::Error::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => return Err("实时节点连接失败".into()),
            _ => {}
        }
        if Instant::now() > deadline {
            return Err("等待实时节点就绪超时".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
}
fn host_owned(url: &url::Url) -> String {
    url.host_str().unwrap_or("").into()
}
fn set_nonblocking(s: &mut Socket) -> Result<(), String> {
    match s.get_mut() {
        MaybeTlsStream::Plain(t) => t.set_nonblocking(true),
        MaybeTlsStream::Rustls(t) => t.sock.set_nonblocking(true),
        _ => return Err("不支持的 TLS 传输".into()),
    }
    .map_err(|_| "无法配置实时连接".into())
}
#[derive(Default)]
struct Timeline {
    offset: u64,
    run: u64,
    usage: BTreeMap<u64, Value>,
    sections: BTreeMap<u64, Vec<Value>>,
}
fn string(v: &Value) -> String {
    v.as_str()
        .map(str::to_owned)
        .or_else(|| v.as_u64().map(|x| x.to_string()))
        .unwrap_or_default()
}
fn digest_text(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        s.to_owned()
    } else if v.is_object() {
        digest_text(
            v.get("text")
                .or_else(|| v.get("content"))
                .unwrap_or(&Value::Null),
        )
    } else {
        String::new()
    }
}
fn normalized_sections(value: &Value) -> Vec<Value> {
    value
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|s| s.is_object())
                .map(|s| {
                    let mut out = s.clone();
                    out["title"] = json!(digest_text(&s["title"]));
                    out["points"] = json!(
                        s["points"]
                            .as_array()
                            .map(|p| p
                                .iter()
                                .map(digest_text)
                                .filter(|p| !p.trim().is_empty())
                                .collect::<Vec<_>>())
                            .unwrap_or_default()
                    );
                    out
                })
                .collect()
        })
        .unwrap_or_default()
}
fn receive(view: &Arc<Mutex<View>>, timeline: &mut Timeline, msg: &Value) {
    mutate(view, |v| match msg["type"].as_str().unwrap_or("") {
        "transcript.partial" => captions::transcript_preview(v, msg, timeline.run),
        "translation.partial" => captions::translation_preview(v, msg, timeline.run),
        "transcript.final" | "translation.final" => {
            let sid = string(&msg["segmentId"]);
            if sid.is_empty() {
                return;
            }
            let id = format!("{}:{sid}", timeline.run);
            let pos = v
                .segments
                .iter()
                .position(|s| s.id == id)
                .unwrap_or_else(|| {
                    v.segments.push(Segment {
                        id: id.clone(),
                        ..Segment::default()
                    });
                    v.segments.len() - 1
                });
            let s = &mut v.segments[pos];
            if let Some(n) = msg["startMs"].as_u64() {
                s.start_ms = Some(n + timeline.offset)
            }
            if let Some(n) = msg["endMs"].as_u64() {
                s.end_ms = Some(n + timeline.offset)
            }
            if !msg["speaker"].is_null() {
                s.speaker = format!("{}.{}", timeline.run, string(&msg["speaker"]))
            }
            if msg["type"] == "transcript.final" {
                s.text = string(&msg["text"]);
            } else {
                if s.text.is_empty() {
                    s.text = string(&msg["sourceText"])
                }
                s.translation = string(&msg["text"]);
                s.enhanced = msg["enhanced"] == true;
            }
            let lang = string(&msg["language"]);
            if !lang.is_empty() {
                s.language = lang
            } else if let Some(l) = msg["sourceLanguage"].as_str() {
                s.language = l.into()
            }
            let displayed = s.clone();
            captions::final_caption(
                v,
                &displayed,
                msg["type"].as_str().unwrap_or(""),
                timeline.run,
            );
            v.segments.sort_by_key(|s| s.start_ms.unwrap_or(u64::MAX));
        }
        "digest.update" => {
            let mut sections = normalized_sections(&msg["sections"]);
            for s in &mut sections {
                for k in ["startMs", "endMs"] {
                    if let Some(n) = s[k].as_u64() {
                        s[k] = json!(n + timeline.offset)
                    }
                }
            }
            timeline.sections.insert(timeline.run, sections);
            v.sections = timeline.sections.values().flatten().cloned().collect();
        }
        "usage.update" => {
            timeline.usage.insert(timeline.run, msg.clone());
            let mut sum = json!({});
            for u in timeline.usage.values() {
                for k in ["effectiveSeconds", "discardedSeconds", "accruedCents"] {
                    if let Some(n) = u[k].as_f64() {
                        sum[k] = json!(sum[k].as_f64().unwrap_or(0.0) + n)
                    }
                }
                for k in ["balanceCents", "priceCentsPerHour"] {
                    if u[k].is_number() {
                        sum[k] = u[k].clone()
                    }
                }
            }
            v.usage = sum;
        }
        "session.error" => v.warning = "LecSync 返回了会话错误，末尾字幕可能不完整".into(),
        _ => {}
    });
}
fn drain(socket: &mut Socket, view: &Arc<Mutex<View>>, t: &mut Timeline) -> Result<bool, String> {
    for _ in 0..64 {
        match socket.read() {
            Ok(Message::Text(text)) => {
                if let Ok(m) = serde_json::from_str::<Value>(&text) {
                    receive(view, t, &m);
                    if m["type"] == "session.ended" {
                        return Ok(true);
                    }
                    if m["type"] == "session.error" && m["scope"] == "session" {
                        return Err("实时会话出现错误，已保留本机录音".into());
                    }
                }
            }
            Ok(Message::Close(_)) => return Ok(true),
            Err(tungstenite::Error::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(tungstenite::Error::ConnectionClosed) => return Ok(true),
            Err(_) => return Err("实时连接已断开，已保留本机录音".into()),
            _ => {}
        }
    }
    Ok(false)
}
fn finish_stream(socket: &mut Option<Socket>, view: &Arc<Mutex<View>>, t: &mut Timeline) {
    if let Some(s) = socket.as_mut() {
        let _ = s.send(Message::Text("{\"type\":\"session.stop\"}".into()));
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let _ = s.flush();
            match drain(s, view, t) {
                Ok(true) => break,
                Err(_) => break,
                _ => {}
            }
            if Instant::now() > deadline {
                mutate(view, |v| {
                    v.warning = "等待末尾字幕超时，已保存当前内容".into()
                });
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = s.close(None);
    }
    *socket = None;
    mutate(view, |v| {
        v.preview.clear();
        v.preview_translation.clear();
    });
}
static FILE_WRITE_LOCK: Mutex<()> = Mutex::new(());
fn save(view: &Arc<Mutex<View>>, directory: &str) -> Result<(), String> {
    // Digest translation and periodic checkpoints share each record's atomic temp file.
    let _writer = FILE_WRITE_LOCK.lock().map_err(|_| "记录保存状态异常")?;
    let v = view.lock().map_err(|_| "字幕状态读取失败")?.clone();
    write_record(&v, directory)
}
// The caller holds FILE_WRITE_LOCK across reading, editing and writing a record.
fn write_record(v: &View, directory: &str) -> Result<(), String> {
    let path = Path::new(directory).join(format!("{}.json", v.id));
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, serde_json::to_vec(&v).map_err(|_| "字幕编码失败")?)
        .map_err(|_| "字幕保存失败")?;
    fs::rename(tmp, path).map_err(|_| String::from("字幕保存失败"))
}
fn worker(
    c: Config,
    ctrl: Receiver<Control>,
    frames: Receiver<Frame>,
    view: Arc<Mutex<View>>,
    wave: Arc<Mutex<Wave>>,
) {
    transport::run(c, ctrl, frames, view, wave);
}
pub fn mix(mic: &[i16], system: &[i16], count: usize) -> (Vec<u8>, f64) {
    let mut bytes = Vec::with_capacity(count * 2);
    let mut sq = 0f64;
    for i in 0..count {
        let a = mic.get(i).copied().unwrap_or(0) as i32;
        let b = system.get(i).copied().unwrap_or(0) as i32;
        let n = if !mic.is_empty() && !system.is_empty() {
            ((a + b) as f64 * 0.707).round() as i32
        } else {
            a + b
        };
        let n = n.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        bytes.extend(n.to_le_bytes());
        sq += (n as f64 / 32768.0).powi(2)
    }
    (bytes, (sq / count.max(1) as f64).sqrt())
}
fn push(mic: &[i16], system: &[i16], count: usize) -> Result<(), String> {
    let guard = ENGINE.lock().map_err(|_| "录音状态异常")?;
    let e = guard.as_ref().ok_or("没有录音会话")?;
    if e.view.lock().map_err(|_| "状态读取失败")?.phase != "recording" {
        return Ok(());
    }
    let (bytes, level) = mix(mic, system, count.min(4096));
    let mut w = e.wave.lock().map_err(|_| "录音状态异常")?;
    w.write(&bytes)?;
    let duration_ms = w.samples * 1000 / RATE;
    mutate(&e.view, |v| {
        v.duration_ms = duration_ms;
        v.level = level
    });
    let start_ms = duration_ms.saturating_sub(bytes.len() as u64 * 1000 / (RATE * 2));
    if e.frames.try_send(Frame { bytes, start_ms }).is_err() {
        mutate(&e.view, |v| {
            v.dropped_frames += 1;
            v.warning = "网络拥塞，字幕可能缺失；本机录音完整保留".into()
        })
    }
    Ok(())
}
fn valid_id(v: &str) -> bool {
    !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit() || b == b'-')
}
fn load(directory: &str, id: &str) -> Result<View, String> {
    if !valid_id(id) {
        return Err("无效记录 ID".into());
    }
    let mut v: View = serde_json::from_slice(
        &fs::read(Path::new(directory).join(format!("{id}.json"))).map_err(|_| "记录不存在")?,
    )
    .map_err(|_| "记录无法读取")?;
    v.sections = normalized_sections(&json!(v.sections));
    Ok(v)
}
pub fn srt(segments: &[Segment]) -> Result<String, String> {
    let mut out = String::new();
    for (i, s) in segments.iter().enumerate() {
        let a = s.start_ms.ok_or("部分字幕缺少时间戳，请导出 TXT")?;
        let b = s.end_ms.ok_or("部分字幕缺少时间戳，请导出 TXT")?;
        out.push_str(&format!(
            "{}\n{} --> {}\n{}\n{}\n\n",
            i + 1,
            timestamp(a),
            timestamp(b.max(a + 1)),
            s.text,
            s.translation
        ));
    }
    Ok(out)
}
fn timestamp(n: u64) -> String {
    format!(
        "{:02}:{:02}:{:02},{:03}",
        n / 3600000,
        n / 60000 % 60,
        n / 1000 % 60,
        n % 1000
    )
}
fn transcript(v: &View) -> String {
    let mut out = format!("{}\n\n", v.title);
    for s in &v.segments {
        out.push_str(&format!(
            "[{}] 说话人 {}\n{}\n{}\n\n",
            timestamp(s.start_ms.unwrap_or(0)),
            s.speaker,
            s.text,
            s.translation
        ))
    }
    let sections = if !v.chinese_sections.is_empty() && v.chinese_digest_source == v.sections {
        &v.chinese_sections
    } else {
        &v.sections
    };
    if !sections.is_empty() {
        out.push_str("纪要\n");
        for s in sections {
            out.push_str(&format!("{}\n", string(&s["title"])));
            if let Some(p) = s["points"].as_array() {
                for p in p {
                    out.push_str(&format!("• {}\n", digest_text(p)))
                }
            }
        }
    }
    out
}
fn command(input: &str) -> Result<Value, String> {
    let req: Value = serde_json::from_str(input).map_err(|_| "无效请求")?;
    let op = req["op"].as_str().unwrap_or("");
    match op {
        "snapshot" => Ok(json!(snapshot())),
        "editCaption" => caption_edits::command(&req),
        "rename" => record_edits::rename(&req),
        "retranslateCaption" => caption_edits::retranslate(&req),
        "warn" | "fail" => {
            let e = ENGINE.lock().map_err(|_| "状态异常")?;
            let message = req["message"].as_str().unwrap_or("录音发生错误");
            if let Some(e) = e.as_ref() {
                mutate(&e.view, |v| {
                    if op == "fail" {
                        v.error = message.into()
                    } else {
                        v.warning = message.into()
                    }
                });
                if op == "fail" {
                    let _ = e.control.send(Control::Stop);
                }
            } else if let Ok(mut v) = LAST.lock() {
                v.error = message.into();
                v.phase = "ended".into();
            }
            Ok(json!({"ok":true}))
        }
        "digestTranslation" => {
            let id = req["id"].as_str().ok_or("缺少记录 ID")?;
            let directory = req["directory"].as_str().ok_or("缺少目录")?;
            let raw = normalized_sections(&req["raw"]);
            let translated = normalized_sections(&req["sections"]);
            let engine = ENGINE.lock().map_err(|_| "状态异常")?;
            if let Some(e) = engine.as_ref() {
                let same = e.view.lock().map_err(|_| "状态异常")?.id == id;
                if same {
                    mutate(&e.view, |v| {
                        if v.sections == raw {
                            v.chinese_sections = translated.clone();
                            v.chinese_digest_source = raw.clone();
                            v.digest_translated = req["translated"] == true;
                        }
                    });
                    let _ = save(&e.view, directory);
                    return Ok(json!({"ok":true}));
                }
            }
            drop(engine);
            let _writer = FILE_WRITE_LOCK.lock().map_err(|_| "记录保存状态异常")?;
            let mut v = load(directory, id)?;
            if v.sections == raw {
                v.chinese_sections = translated;
                v.chinese_digest_source = raw;
                v.digest_translated = req["translated"] == true;
                write_record(&v, directory)?;
                if let Ok(mut last) = LAST.lock() {
                    if last.id == id {
                        *last = v;
                    }
                }
            }
            Ok(json!({"ok":true}))
        }
        "start" => {
            let c: Config =
                serde_json::from_value(req["config"].clone()).map_err(|_| "无效录音配置")?;
            languages::parse_pair(&c.pair)?;
            let mut engine = ENGINE.lock().map_err(|_| "状态异常")?;
            if let Some(e) = engine.as_ref() {
                if e.view.lock().map_err(|_| "状态异常")?.phase != "ended" {
                    return Err("已有录音正在运行".into());
                }
            }
            fs::create_dir_all(&c.directory).map_err(|_| "无法创建历史目录")?;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| "系统时间异常")?
                .as_millis() as u64;
            let id = now.to_string();
            let wave = Arc::new(Mutex::new(Wave::new(
                Path::new(&c.directory).join(format!("{id}.wav")),
            )?));
            let view = Arc::new(Mutex::new(View {
                id: id.clone(),
                title: if c.title.trim().is_empty() {
                    "未命名录音".into()
                } else {
                    c.title.clone()
                },
                created_at: now,
                source: c.source.clone(),
                pair: c.pair.clone(),
                phase: "recording".into(),
                connection: "connecting".into(),
                audio_path: wave
                    .lock()
                    .map_err(|_| "录音文件错误")?
                    .path
                    .to_string_lossy()
                    .into(),
                usage: json!({}),
                ..View::default()
            }));
            let (tx, rx) = mpsc::channel();
            let (ftx, frx) = mpsc::sync_channel(100);
            let v = view.clone();
            let w = wave.clone();
            thread::spawn(move || {
                let recovery_view = v.clone();
                let recovery_wave = w.clone();
                let directory = c.directory.clone();
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    worker(c, rx, frx, v, w)
                }))
                .is_err()
                {
                    if let Ok(mut wave) = recovery_wave.lock() {
                        let _ = wave.header();
                    }
                    mutate(&recovery_view, |v| {
                        v.phase = "ended".into();
                        v.error = "录音线程发生异常，已保留最后保存的内容".into();
                    });
                    let _ = save(&recovery_view, &directory);
                }
            });
            *engine = Some(Engine {
                control: tx,
                frames: ftx,
                view,
                wave,
            });
            Ok(json!({"ok":true}))
        }
        "pause" | "resume" | "stop" => {
            let e = ENGINE.lock().map_err(|_| "状态异常")?;
            if let Some(e) = e.as_ref() {
                let mut v = e.view.lock().map_err(|_| "状态异常")?;
                let p = v.phase.clone();
                let control = match op {
                    "pause" if p == "recording" => {
                        v.phase = "pausing".into();
                        Control::Pause
                    }
                    "resume" if p == "paused" => {
                        v.phase = "recording".into();
                        v.connection = "connecting".into();
                        Control::Resume
                    }
                    "stop" if p != "ended" => {
                        v.phase = "stopping".into();
                        Control::Stop
                    }
                    _ => return Ok(json!({"ok":true})),
                };
                drop(v);
                e.control.send(control).map_err(|_| "录音已结束")?;
            }
            Ok(json!({"ok":true}))
        }
        "test" => {
            let c: Config =
                serde_json::from_value(req["config"].clone()).map_err(|_| "无效配置")?;
            let mut s = open(&c)?;
            let _ = s.send(Message::Text("{\"type\":\"session.stop\"}".into()));
            let _ = s.close(None);
            Ok(json!({"ok":true,"message":"连接成功，未采集或发送音频"}))
        }
        "list" => {
            let directory = req["directory"].as_str().ok_or("缺少历史目录")?;
            let mut records = Vec::new();
            if let Ok(entries) = fs::read_dir(directory) {
                let active = snapshot();
                for e in entries.flatten() {
                    if e.path().extension().and_then(|x| x.to_str()) == Some("json") {
                        if let Ok(bytes) = fs::read(e.path()) {
                            if let Ok(mut v) = serde_json::from_slice::<View>(&bytes) {
                                if v.id == active.id
                                    && !["idle", "ended"].contains(&active.phase.as_str())
                                {
                                    continue;
                                }
                                v.sections = normalized_sections(&json!(v.sections));
                                if v.phase != "ended" {
                                    v.phase = "ended".into();
                                    v.warning = "上次录音中断，已恢复最后保存的内容".into();
                                }
                                records.push(caption_edits::render(&v))
                            }
                        }
                    }
                }
            }
            records.sort_by_key(|v| std::cmp::Reverse(v.created_at));
            Ok(json!(records))
        }
        "load" | "export" | "delete" => {
            let directory = req["directory"].as_str().ok_or("缺少目录")?;
            let id = req["id"].as_str().ok_or("缺少记录 ID")?;
            let v = load(directory, id)?;
            if op == "load" {
                return Ok(json!(caption_edits::render(&v)));
            }
            if op == "delete" {
                if snapshot().id == id && !["idle", "ended"].contains(&snapshot().phase.as_str()) {
                    return Err("不能删除正在录制的记录".into());
                }
                let _ = fs::remove_file(Path::new(directory).join(format!("{id}.wav")));
                fs::remove_file(Path::new(directory).join(format!("{id}.json")))
                    .map_err(|_| "删除失败")?;
                return Ok(json!({"ok":true}));
            }
            let format = req["format"].as_str().unwrap_or("txt");
            let v = caption_edits::render(&v);
            let content = match format {
                "srt" => srt(&v.segments)?,
                "json" => serde_json::to_string_pretty(&v).map_err(|_| "导出失败")?,
                _ => transcript(&v),
            };
            Ok(
                json!({"content":content,"audioPath":v.audio_path,"filename":format!("听译台-{}.{}",v.id,format)}),
            )
        }
        _ => Err("未知操作".into()),
    }
}
fn response(input: &str) -> String {
    match std::panic::catch_unwind(|| command(input)) {
        Ok(Ok(v)) => v.to_string(),
        Ok(Err(e)) => json!({"error":e}).to_string(),
        Err(_) => json!({"error":"底层发生异常，已阻止异常跨越 JNI"}).to_string(),
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_lecsync_desk_NativeCore_command(
    mut env: JNIEnv,
    _: JObject,
    input: JString,
) -> jstring {
    let input: String = match env.get_string(&input) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    env.new_string(response(&input))
        .map(|x| x.into_raw())
        .unwrap_or(std::ptr::null_mut())
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_lecsync_desk_NativeCore_push(
    env: JNIEnv,
    _: JObject,
    mic: JShortArray,
    system: JShortArray,
    count: jint,
) -> jint {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !(1..=4096).contains(&count) {
            return Err("无效音频帧".into());
        }
        let ml = env.get_array_length(&mic).map_err(|_| "无效麦克风缓冲区")? as usize;
        let sl = env
            .get_array_length(&system)
            .map_err(|_| "无效系统音频缓冲区")? as usize;
        let mut m = vec![0i16; ml.min(count as usize)];
        let mut s = vec![0i16; sl.min(count as usize)];
        if !m.is_empty() {
            env.get_short_array_region(&mic, 0, &mut m)
                .map_err(|_| "麦克风读取失败")?
        }
        if !s.is_empty() {
            env.get_short_array_region(&system, 0, &mut s)
                .map_err(|_| "系统音频读取失败")?
        }
        push(&m, &s, count as usize)
    }));
    match result {
        Ok(Ok(())) => 0,
        Ok(Err(e)) => {
            if let Ok(g) = ENGINE.lock() {
                if let Some(x) = g.as_ref() {
                    mutate(&x.view, |v| v.error = e);
                    let _ = x.control.send(Control::Stop);
                }
            }
            -1
        }
        Err(_) => -1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concurrent_digest_and_record_checkpoints_are_atomic() {
        let directory = std::env::temp_dir().join(format!("lecsync-save-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let shared = Arc::new(Mutex::new(View {
            id: "9001".into(),
            title: "完整的录音记录".repeat(2000),
            ..View::idle()
        }));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let shared = shared.clone();
                let directory = directory.clone();
                thread::spawn(move || {
                    for _ in 0..8 {
                        save(&shared, directory.to_str().unwrap()).unwrap();
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        let restored = load(directory.to_str().unwrap(), "9001").unwrap();
        assert_eq!(restored.title, shared.lock().unwrap().title);
        fs::remove_file(directory.join("9001.json")).unwrap();
        fs::remove_dir(directory).unwrap();
    }
    #[test]
    fn legacy_record_and_structured_digest_remain_readable() {
        let raw = json!({"id":"1001","phase":"ended","durationMs":1000,"segments":[],
            "sections":[{"title":"会議", "points":[{"id":"s1-p1","text":"要点"},"第二点",{"id":"empty"}]}]});
        let mut view: View = serde_json::from_value(raw).unwrap();
        assert!(view.captions.is_empty());
        view.sections = normalized_sections(&json!(view.sections));
        assert_eq!(view.sections[0]["points"], json!(["要点", "第二点"]));
        let exported = transcript(&view);
        assert!(exported.contains("要点"));
        assert!(!exported.contains("s1-p1"));
    }
    #[test]
    fn export_uses_matching_chinese_digest_only() {
        let mut view = View::idle();
        view.sections = normalized_sections(&json!([{"title":"Meeting","points":["Original"]}]));
        view.chinese_sections =
            normalized_sections(&json!([{"title":"会议","points":["中文要点"]}]));
        view.chinese_digest_source = view.sections.clone();
        assert!(transcript(&view).contains("中文要点"));
        view.sections[0]["title"] = json!("New meeting");
        assert!(transcript(&view).contains("New meeting"));
        assert!(!transcript(&view).contains("中文要点"));
    }
    #[test]
    fn pcm_is_little_endian_and_mix_clips() {
        let (b, _) = mix(&[32767, -32768], &[32767, -32768], 2);
        assert_eq!(b, vec![255, 127, 0, 128]);
    }
    #[test]
    fn microphone_only_preserves_samples() {
        let (b, _) = mix(&[1234, -2345], &[], 2);
        assert_eq!(
            b,
            [1234i16.to_le_bytes(), (-2345i16).to_le_bytes()].concat()
        );
    }
    #[test]
    fn late_translation_updates_same_row() {
        let v = Arc::new(Mutex::new(View::idle()));
        let mut t = Timeline {
            run: 1,
            ..Timeline::default()
        };
        receive(
            &v,
            &mut t,
            &json!({"type":"translation.final","segmentId":"a","sourceText":"hello","text":"你好","startMs":0,"endMs":900}),
        );
        receive(
            &v,
            &mut t,
            &json!({"type":"transcript.final","segmentId":"a","text":"Hello","startMs":0,"endMs":950}),
        );
        let v = v.lock().unwrap();
        assert_eq!(v.segments.len(), 1);
        assert_eq!(v.segments[0].translation, "你好");
    }
    #[test]
    fn resume_offsets_ids_time_and_usage() {
        let v = Arc::new(Mutex::new(View::idle()));
        let mut t = Timeline {
            run: 1,
            ..Timeline::default()
        };
        receive(
            &v,
            &mut t,
            &json!({"type":"transcript.final","segmentId":"a","text":"one","startMs":0,"endMs":100}),
        );
        receive(
            &v,
            &mut t,
            &json!({"type":"usage.update","effectiveSeconds":10,"accruedCents":2}),
        );
        t.run = 2;
        t.offset = 10000;
        receive(
            &v,
            &mut t,
            &json!({"type":"transcript.final","segmentId":"a","text":"two","startMs":100,"endMs":200}),
        );
        receive(
            &v,
            &mut t,
            &json!({"type":"usage.update","effectiveSeconds":5,"accruedCents":1}),
        );
        let v = v.lock().unwrap();
        assert_eq!(v.segments[1].start_ms, Some(10100));
        assert_eq!(v.usage["effectiveSeconds"], 15.0);
        assert_eq!(v.usage["accruedCents"], 3.0);
    }
    #[test]
    fn srt_never_invents_timestamps() {
        assert!(srt(&[Segment::default()]).is_err());
    }
    #[test]
    fn wave_header_matches_samples() {
        let path = std::env::temp_dir().join(format!("lecsync-test-{}.wav", std::process::id()));
        {
            let mut w = Wave::new(path.clone()).unwrap();
            w.write(&vec![0; 640]).unwrap();
            w.header().unwrap();
        }
        let b = fs::read(&path).unwrap();
        assert_eq!(b.len(), 684);
        assert_eq!(&b[40..44], &640u32.to_le_bytes());
        let _ = fs::remove_file(path);
    }
}
