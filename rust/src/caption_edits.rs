//! Manual corrections are an overlay: matching continues to use the original stream.
use super::record_edits::update;
#[cfg(test)]
use super::record_edits::update_shared;
use super::*;

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct Edits {
    caption: BTreeMap<String, Fields>,
    segment: BTreeMap<String, Fields>,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Fields {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    translation: Option<String>,
    revision: u64,
    translation_job: Option<text_translation::Job>,
}

impl Fields {
    fn apply(&self, segment: &mut Segment) {
        if let Some(text) = &self.text {
            segment.text.clone_from(text);
        }
        if let Some(translation) = &self.translation {
            segment.translation.clone_from(translation);
        }
    }
}

struct Edit<'a> {
    row_id: &'a str,
    row_type: &'a str,
    field: &'a str,
    value: &'a str,
}

impl<'a> Edit<'a> {
    fn parse(req: &'a Value) -> Result<Self, String> {
        let edit = Self {
            row_id: req["rowId"].as_str().ok_or("缺少字幕 ID")?,
            row_type: req["rowType"].as_str().ok_or("缺少字幕类型")?,
            field: req["field"].as_str().ok_or("缺少编辑字段")?,
            value: req["value"].as_str().ok_or("无效字幕内容")?,
        };
        if !["caption", "segment"].contains(&edit.row_type) {
            return Err("无效字幕类型".into());
        }
        if !["text", "translation"].contains(&edit.field) {
            return Err("只能编辑原文或译文".into());
        }
        Ok(edit)
    }

    fn apply(&self, v: &mut View) -> Result<(), String> {
        let exists = match self.row_type {
            "caption" => v.captions.iter().any(|r| r.segment.id == self.row_id),
            "segment" => v.segments.iter().any(|s| s.id == self.row_id),
            _ => false,
        };
        if !exists {
            return Err("所选字幕已不存在，请重新打开记录".into());
        }
        let edits = if self.row_type == "caption" {
            &mut v.caption_edits.caption
        } else {
            &mut v.caption_edits.segment
        };
        let fields = edits.entry(self.row_id.to_owned()).or_default();
        if self.field == "text" {
            if fields.text.as_deref() != Some(self.value) {
                fields.text = Some(self.value.to_owned());
                fields.revision = fields.revision.saturating_add(1);
                fields.translation_job = None;
            }
        } else {
            fields.translation = Some(self.value.to_owned());
            fields.revision = fields.revision.saturating_add(1);
            fields.translation_job = None;
        }
        Ok(())
    }
}

/// Public snapshots are rendered copies. Serialized checkpoints retain raw matching data.
pub(super) fn render(raw: &View) -> View {
    let mut rendered = raw.clone();
    if raw.caption_edits.caption.is_empty() && raw.caption_edits.segment.is_empty() {
        return rendered;
    }
    let mut counts = BTreeMap::new();
    for caption in &raw.captions {
        if let Some(id) = &caption.segment_id {
            *counts.entry(id.as_str()).or_insert(0) += 1;
        }
    }
    for caption in &mut rendered.captions {
        // Segment edits are a fallback for old records without caption rows. If such
        // a record gains one matching row, continue to show its saved correction.
        if let Some(segment_id) = &caption.segment_id {
            if counts.get(segment_id.as_str()) == Some(&1) {
                if let Some(edit) = raw.caption_edits.segment.get(segment_id) {
                    edit.apply(&mut caption.segment);
                }
            }
        }
        if let Some(edit) = raw.caption_edits.caption.get(&caption.segment.id) {
            edit.apply(&mut caption.segment);
        }
    }
    if !raw.preview.is_empty() {
        rendered.preview = rendered
            .captions
            .iter()
            .filter(|r| r.provisional)
            .map(|r| r.segment.text.as_str())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
    }
    if !raw.preview_translation.is_empty() {
        rendered.preview_translation = rendered
            .captions
            .iter()
            .filter(|r| r.provisional)
            .map(|r| r.segment.translation.as_str())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
    }

    let mut by_segment: BTreeMap<&str, Vec<&captions::Caption>> = BTreeMap::new();
    for caption in &rendered.captions {
        if let Some(id) = &caption.segment_id {
            by_segment.entry(id).or_default().push(caption);
        }
    }
    let segment_ids: std::collections::BTreeSet<_> =
        raw.segments.iter().map(|s| s.id.as_str()).collect();
    let mut segments = Vec::new();
    for segment in &raw.segments {
        let rows = by_segment
            .get(segment.id.as_str())
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if rows
            .iter()
            .any(|r| raw.caption_edits.caption.contains_key(&r.segment.id))
        {
            // A final segment can span multiple display rows. Export those rows
            // separately so editing one never duplicates or overwrites its neighbours.
            for row in rows {
                let mut s = row.segment.clone();
                s.start_ms = s.start_ms.or(segment.start_ms);
                s.end_ms = s.end_ms.or(segment.end_ms);
                if s.speaker.is_empty() {
                    s.speaker.clone_from(&segment.speaker);
                }
                segments.push(s);
            }
        } else {
            let mut s = segment.clone();
            if let Some(edit) = raw.caption_edits.segment.get(&s.id) {
                edit.apply(&mut s);
            }
            segments.push(s);
        }
    }
    // An edited pending caption must also survive TXT/JSON export. SRT continues
    // to reject missing timestamps rather than fabricating a time interval.
    for row in &rendered.captions {
        if raw.caption_edits.caption.contains_key(&row.segment.id)
            && row
                .segment_id
                .as_deref()
                .is_none_or(|id| !segment_ids.contains(id))
        {
            segments.push(row.segment.clone());
        }
    }
    segments.sort_by_key(|s| s.start_ms.unwrap_or(u64::MAX));
    rendered.segments = segments;
    rendered
}

#[cfg(test)]
fn apply_shared(view: &Arc<Mutex<View>>, edit: &Edit<'_>, directory: &str) -> Result<View, String> {
    update_shared(view, directory, &mut |v| edit.apply(v)).map(|v| render(&v))
}

pub(super) fn command(req: &Value) -> Result<Value, String> {
    let edit = Edit::parse(req)?;
    update(req, |v| edit.apply(v)).map(|v| json!(render(&v)))
}

fn fields<'a>(v: &'a mut View, row_type: &str, row_id: &str) -> Result<&'a mut Fields, String> {
    let map = match row_type {
        "caption" if v.captions.iter().any(|r| r.segment.id == row_id) => {
            &mut v.caption_edits.caption
        }
        "segment" if v.segments.iter().any(|s| s.id == row_id) => &mut v.caption_edits.segment,
        _ => return Err("所选字幕已不存在，请重新打开记录".into()),
    };
    Ok(map.entry(row_id.to_owned()).or_default())
}

fn language_hint(language: &str) -> String {
    let lower = language.to_ascii_lowercase();
    match lower.split(['-', '_']).next().unwrap_or("") {
        "nb" | "nn" => "no".into(),
        "fil" => "tl".into(),
        "iw" => "he".into(),
        code => code.into(),
    }
}

fn prepare(v: &mut View, req: &Value) -> Result<Option<text_translation::Job>, String> {
    let row_type = req["rowType"].as_str().unwrap_or("");
    let row_id = req["rowId"].as_str().unwrap_or("");
    let rendered = render(v);
    let row = match row_type {
        "caption" => rendered
            .captions
            .iter()
            .map(|r| &r.segment)
            .find(|r| r.id == row_id),
        "segment" => rendered.segments.iter().find(|r| r.id == row_id),
        _ => None,
    }
    .ok_or("所选字幕已不存在，请重新打开记录")?;
    let pair = languages::parse_pair(if v.pair.is_empty() { "ja-zh" } else { &v.pair })?;
    let hint = language_hint(&row.language);
    let target = req["targetLanguage"]
        .as_str()
        .filter(|s| pair.contains(s))
        .ok_or("不支持的语言")?;
    let source = if pair.contains(&hint.as_str()) && hint != target {
        hint.as_str()
    } else {
        "auto"
    };
    let fields = fields(v, row_type, row_id)?;
    if row.text.trim().is_empty() {
        fields.translation = Some(String::new());
        fields.translation_job = None;
        fields.revision = fields.revision.saturating_add(1);
        return Ok(None);
    }
    if row.text.chars().count() > 2000 {
        return Err("原文超过 2,000 字符，无法自动重译。".into());
    }
    if let Some(job) = &fields.translation_job {
        if job.text == row.text
            && job.source_language == source
            && job.target_language == target
            && job.revision == fields.revision
            && !job.terminal_failure
        {
            return Ok(if job.succeeded {
                None
            } else {
                Some(job.clone())
            });
        }
    }
    let job = text_translation::Job {
        text: row.text.clone(),
        source_language: source.into(),
        target_language: target.into(),
        key: text_translation::new_key(),
        revision: fields.revision,
        ..text_translation::Job::default()
    };
    fields.translation_job = Some(job.clone());
    Ok(Some(job))
}

fn current_job<'a>(
    v: &'a mut View,
    req: &Value,
    expected: &text_translation::Job,
) -> Result<&'a mut Fields, String> {
    let rendered = render(v);
    let row_id = req["rowId"].as_str().unwrap_or("");
    let row = if req["rowType"] == "caption" {
        rendered
            .captions
            .iter()
            .map(|r| &r.segment)
            .find(|s| s.id == row_id)
    } else {
        rendered.segments.iter().find(|s| s.id == row_id)
    };
    if row.is_none_or(|s| s.text != expected.text) {
        return Err("字幕已有新的修改，请重新打开后重试。".into());
    }
    let fields = fields(
        v,
        req["rowType"].as_str().unwrap_or(""),
        req["rowId"].as_str().unwrap_or(""),
    )?;
    if fields.revision != expected.revision
        || fields
            .translation_job
            .as_ref()
            .is_none_or(|j| j.key != expected.key)
    {
        return Err("字幕已有新的修改，请重新打开后重试。".into());
    }
    Ok(fields)
}

pub(super) fn retranslate(req: &Value) -> Result<Value, String> {
    retranslate_using(req, |key, job, remember| {
        text_translation::translate(key, job, remember)
    })
}

fn retranslate_using(
    req: &Value,
    translate: impl FnOnce(
        &str,
        &text_translation::Job,
        &mut dyn FnMut(&str) -> Result<(), String>,
    ) -> Result<String, text_translation::Failure>,
) -> Result<Value, String> {
    let mut job = None;
    let prepared = update(req, |v| {
        job = prepare(v, req)?;
        Ok(())
    })?;
    let Some(job) = job else {
        return Ok(json!(render(&prepared)));
    };
    let outcome = translate(
        req["apiKey"].as_str().unwrap_or(""),
        &job,
        &mut |id: &str| {
            update(req, |v| {
                current_job(v, req, &job)?
                    .translation_job
                    .as_mut()
                    .unwrap()
                    .request_id = Some(id.into());
                Ok(())
            })
            .map(|_| ())
        },
    );
    let mut error = None;
    let updated = update(req, |v| {
        // If another editor has saved new text or a manual translation, an old
        // response must never overwrite it, even when requests finish out of order.
        let fields = match current_job(v, req, &job) {
            Ok(fields) => fields,
            Err(message) => {
                error = Some(message);
                return Ok(());
            }
        };
        match &outcome {
            Ok(text) => {
                fields.translation = Some(text.clone());
                fields.translation_job.as_mut().unwrap().succeeded = true;
            }
            Err(failure) => {
                fields.translation_job.as_mut().unwrap().terminal_failure = failure.terminal;
                error = Some(failure.message.clone());
            }
        }
        Ok(())
    })?;
    let mut output = json!(render(&updated));
    if let Some(error) = error {
        output["retranslationError"] = json!(error);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let n = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("lingodesk-edits-{}-{n}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> &str {
            self.0.to_str().unwrap()
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn fixture(id: &str) -> View {
        let s = Segment {
            id: "1:server-row".into(),
            text: "Original source".into(),
            translation: "Original translation".into(),
            start_ms: Some(1200),
            end_ms: Some(2600),
            speaker: "1.A".into(),
            ..Segment::default()
        };
        let mut caption = captions::Caption {
            segment: s.clone(),
            segment_id: Some(s.id.clone()),
            run: 1,
            ..captions::Caption::default()
        };
        caption.segment.id = "caption-1".into();
        View {
            id: id.into(),
            phase: "recording".into(),
            captions: vec![caption],
            segments: vec![s],
            ..View::idle()
        }
    }

    fn request(
        id: &str,
        directory: &Directory,
        row_type: &str,
        row_id: &str,
        field: &str,
        value: &str,
    ) -> Value {
        json!({"op":"editCaption", "id":id, "directory":directory.path(),
            "rowType":row_type, "rowId":row_id, "field":field, "value":value})
    }

    fn translation_request(directory: &Directory, id: &str) -> Value {
        let mut v = fixture(id);
        v.pair = "en-zh".into();
        v.captions[0].segment.language = "en-US".into();
        fs::write(
            directory.0.join(format!("{id}.json")),
            serde_json::to_vec(&v).unwrap(),
        )
        .unwrap();
        command(&request(
            id,
            directory,
            "caption",
            "caption-1",
            "text",
            "Corrected source",
        ))
        .unwrap();
        json!({"id":id,"directory":directory.path(),"rowType":"caption","rowId":"caption-1","targetLanguage":"zh","apiKey":"test-key"})
    }

    #[test]
    fn retranslation_is_persisted_exported_and_cached_for_unchanged_saves() {
        let directory = Directory::new();
        let req = translation_request(&directory, "9301");
        let output = retranslate_using(&req, |key, job, remember| {
            assert_eq!(key, "test-key");
            assert_eq!(job.text, "Corrected source");
            assert_eq!(job.source_language, "en");
            assert_eq!(job.target_language, "zh");
            remember("req_saved").unwrap();
            let stored = load(directory.path(), "9301").unwrap();
            assert_eq!(
                stored.caption_edits.caption["caption-1"]
                    .translation_job
                    .as_ref()
                    .unwrap()
                    .request_id
                    .as_deref(),
                Some("req_saved")
            );
            Ok("修正后的译文".into())
        })
        .unwrap();
        assert_eq!(output["captions"][0]["text"], "Corrected source");
        assert_eq!(output["captions"][0]["translation"], "修正后的译文");
        let restored = render(&load(directory.path(), "9301").unwrap());
        assert!(transcript(&restored).contains("修正后的译文"));
        assert!(srt(&restored.segments).unwrap().contains("修正后的译文"));
        assert!(
            !serde_json::to_string(&restored)
                .unwrap()
                .contains("test-key")
        );
        command(&request(
            "9301",
            &directory,
            "caption",
            "caption-1",
            "text",
            "Corrected source",
        ))
        .unwrap();
        retranslate_using(&req, |_, _, _| {
            panic!("unchanged successful edit must not charge again")
        })
        .unwrap();
    }

    #[test]
    fn interrupted_translation_reuses_persisted_request_and_retains_previous_translation() {
        let directory = Directory::new();
        let req = translation_request(&directory, "9302");
        let mut original_key = String::new();
        let first = retranslate_using(&req, |_, job, remember| {
            original_key = job.key.clone();
            remember("req_pending").unwrap();
            Err(text_translation::Failure {
                message: "连接 LecSync 失败，请检查网络".into(),
                terminal: false,
            })
        })
        .unwrap();
        assert_eq!(first["captions"][0]["text"], "Corrected source");
        assert_eq!(first["captions"][0]["translation"], "Original translation");
        assert!(first["retranslationError"].is_string());
        command(&request(
            "9302",
            &directory,
            "caption",
            "caption-1",
            "text",
            "Corrected source",
        ))
        .unwrap();
        let retried = retranslate_using(&req, |_, job, _| {
            assert_eq!(job.key, original_key);
            assert_eq!(job.request_id.as_deref(), Some("req_pending"));
            Ok("完成".into())
        })
        .unwrap();
        assert_eq!(retried["captions"][0]["translation"], "完成");
    }

    #[test]
    fn delayed_translation_never_overwrites_new_source_or_manual_translation() {
        for field in ["text", "translation"] {
            let directory = Directory::new();
            let req = translation_request(&directory, "9303");
            let output = retranslate_using(&req, |_, _, remember| {
                remember("req_delayed").unwrap();
                command(&request(
                    "9303",
                    &directory,
                    "caption",
                    "caption-1",
                    field,
                    "Newer manual edit",
                ))
                .unwrap();
                Ok("Outdated result".into())
            })
            .unwrap();
            assert_eq!(output["captions"][0][field], "Newer manual edit");
            assert!(output["retranslationError"].is_string());
            assert!(
                !transcript(&render(&load(directory.path(), "9303").unwrap()))
                    .contains("Outdated result")
            );
        }
    }

    #[test]
    fn target_change_invalidates_pending_result_and_terminal_failure_allows_fresh_request() {
        let directory = Directory::new();
        let req = translation_request(&directory, "9304");
        let mut old_key = String::new();
        retranslate_using(&req, |_, job, _| {
            old_key = job.key.clone();
            Err(text_translation::Failure {
                message: "API 钱包余额不足".into(),
                terminal: true,
            })
        })
        .unwrap();
        retranslate_using(&req, |_, job, _| {
            assert_ne!(job.key, old_key);
            let mut other_target = req.clone();
            other_target["targetLanguage"] = json!("en");
            update(&req, |v| {
                prepare(v, &other_target)?;
                Ok(())
            })
            .unwrap();
            Ok("Old target result".into())
        })
        .unwrap();
        let restored = render(&load(directory.path(), "9304").unwrap());
        assert_eq!(
            restored.captions[0].segment.translation,
            "Original translation"
        );
    }

    #[test]
    fn blank_edits_clear_translation_and_unicode_limit_keeps_edits_without_request() {
        let directory = Directory::new();
        let req = translation_request(&directory, "9305");
        for value in [" \n ".to_owned(), "字".repeat(2001)] {
            command(&request(
                "9305",
                &directory,
                "caption",
                "caption-1",
                "text",
                &value,
            ))
            .unwrap();
            let result = retranslate_using(&req, |_, _, _| {
                panic!("blank or oversized text must not be billed")
            });
            let restored = render(&load(directory.path(), "9305").unwrap());
            assert_eq!(restored.captions[0].segment.text, value);
            if value.trim().is_empty() {
                result.unwrap();
                assert_eq!(restored.captions[0].segment.translation, "");
            } else {
                assert!(result.unwrap_err().contains("2,000"));
            }
        }
    }

    #[test]
    fn edits_survive_late_stream_results_and_persist_without_changing_matching_data() {
        let directory = Directory::new();
        let shared = Arc::new(Mutex::new(View {
            id: "9201".into(),
            phase: "recording".into(),
            ..View::idle()
        }));
        let mut timeline = Timeline {
            run: 1,
            ..Timeline::default()
        };
        receive(
            &shared,
            &mut timeline,
            &json!({"type":"transcript.partial","text":"hello"}),
        );
        let req = request(
            "9201",
            &directory,
            "caption",
            "caption-1",
            "text",
            " User correction\n ",
        );
        let rendered =
            apply_shared(&shared, &Edit::parse(&req).unwrap(), directory.path()).unwrap();
        assert_eq!(rendered.captions[0].segment.text, " User correction\n ");
        assert_eq!(shared.lock().unwrap().captions[0].segment.text, "hello");
        receive(
            &shared,
            &mut timeline,
            &json!({"type":"transcript.final","segmentId":"hello","text":"Hello, world","startMs":100,"endMs":1200,"speaker":"A"}),
        );
        receive(
            &shared,
            &mut timeline,
            &json!({"type":"translation.final","segmentId":"hello","sourceText":"Hello, world","text":"你好，世界"}),
        );
        let rendered = render(&shared.lock().unwrap());
        assert_eq!(rendered.captions.len(), 1);
        assert_eq!(rendered.captions[0].segment.text, " User correction\n ");
        assert_eq!(rendered.captions[0].segment.translation, "你好，世界");
        let req = request(
            "9201",
            &directory,
            "caption",
            "caption-1",
            "translation",
            "人工译文",
        );
        apply_shared(&shared, &Edit::parse(&req).unwrap(), directory.path()).unwrap();
        receive(
            &shared,
            &mut timeline,
            &json!({"type":"translation.final","segmentId":"hello","text":"新机器译文"}),
        );
        save(&shared, directory.path()).unwrap();
        let reloaded = load(directory.path(), "9201").unwrap();
        assert_eq!(reloaded.captions[0].segment.text, "Hello, world");
        assert_eq!(reloaded.captions[0].segment.translation, "新机器译文");
        let rendered = render(&reloaded);
        assert_eq!(rendered.captions[0].segment.text, " User correction\n ");
        assert_eq!(rendered.captions[0].segment.translation, "人工译文");
        assert_eq!(rendered.segments[0].start_ms, Some(100));
        assert_eq!(rendered.segments[0].speaker, "1.A");
    }

    #[test]
    fn shared_final_segment_exports_only_the_chosen_caption_edit() {
        let directory = Directory::new();
        let mut view = fixture("9202");
        view.segments[0].text = "First. Second.".into();
        view.segments[0].translation = "一。二。".into();
        view.captions[0].segment.text = "First.".into();
        view.captions[0].segment.translation = "一。".into();
        let mut second = view.captions[0].clone();
        second.segment.id = "caption-2".into();
        second.segment.text = "Second.".into();
        second.segment.translation = "二。".into();
        view.captions.push(second);
        let req = request(
            "9202",
            &directory,
            "caption",
            "caption-1",
            "text",
            "Edited first.",
        );
        Edit::parse(&req).unwrap().apply(&mut view).unwrap();
        let rendered = render(&view);
        assert_eq!(view.segments[0].text, "First. Second.");
        assert_eq!(rendered.captions[0].segment.text, "Edited first.");
        assert_eq!(rendered.captions[1].segment.text, "Second.");
        assert_eq!(rendered.segments.len(), 2);
        assert_eq!(rendered.segments[0].translation, "一。");
        assert_eq!(rendered.segments[1].text, "Second.");
        assert!(
            rendered
                .segments
                .iter()
                .all(|s| s.start_ms == Some(1200) && s.end_ms == Some(2600) && s.speaker == "1.A")
        );
        let text = transcript(&rendered);
        assert!(text.contains("Edited first."));
        assert!(text.contains("Second."));
        assert!(!text.contains("First. Second."));
        let subtitles = srt(&rendered.segments).unwrap();
        assert!(subtitles.contains("00:00:01,200 --> 00:00:02,600"));
        assert!(subtitles.contains("Edited first."));
        let json = serde_json::to_value(&rendered).unwrap();
        assert_eq!(json["segments"][1]["text"], "Second.");
        let mut final_segment = view.segments[0].clone();
        final_segment.translation = "新的第一句。新的第二句。".into();
        captions::final_caption(&mut view, &final_segment, "translation.final", 1);
        let rendered = render(&view);
        assert_eq!(rendered.captions.len(), 2);
        assert_eq!(rendered.captions[0].segment.text, "Edited first.");
        assert_eq!(rendered.captions[1].segment.text, "Second.");
        assert_eq!(rendered.captions[1].segment.translation, "新的第二句。");
    }

    #[test]
    fn historical_legacy_rows_load_list_and_export_edits_and_reject_invalid_requests() {
        let directory = Directory::new();
        let mut original = fixture("9203");
        original.phase = "ended".into();
        original.captions.clear();
        // Legacy files do not have a captionEdits member.
        let mut raw = serde_json::to_value(&original).unwrap();
        raw.as_object_mut().unwrap().remove("captionEdits");
        fs::write(directory.0.join("9203.json"), raw.to_string()).unwrap();
        let req = request(
            "9203",
            &directory,
            "segment",
            "1:server-row",
            "translation",
            "",
        );
        let output = super::super::command(&req.to_string()).unwrap();
        assert_eq!(output["id"], "9203");
        assert_eq!(output["segments"][0]["text"], "Original source");
        assert_eq!(output["segments"][0]["translation"], "");
        let restored = super::super::command(
            &json!({"op":"load","directory":directory.path(),"id":"9203"}).to_string(),
        )
        .unwrap();
        assert_eq!(restored["segments"][0]["translation"], "");
        let list =
            super::super::command(&json!({"op":"list","directory":directory.path()}).to_string())
                .unwrap();
        assert_eq!(list[0]["segments"][0]["translation"], "");
        super::super::command(
            &json!({"op":"digestTranslation", "directory":directory.path(), "id":"9203", "raw":[],
                "sections":[{"title":"纪要","points":["新的纪要"]}], "translated":true})
            .to_string(),
        )
        .unwrap();
        for format in ["txt", "srt", "json"] {
            let exported = super::super::command(
                &json!({"op":"export","directory":directory.path(),"id":"9203","format":format})
                    .to_string(),
            )
            .unwrap();
            assert!(
                !exported["content"]
                    .as_str()
                    .unwrap()
                    .contains("Original translation")
            );
        }
        for (row_type, row_id, field) in [
            ("segment", "missing", "text"),
            ("caption", "caption-1", "text"),
            ("unknown", "1:server-row", "text"),
            ("segment", "1:server-row", "speaker"),
        ] {
            let req = request("9203", &directory, row_type, row_id, field, "bad");
            assert!(super::super::command(&req.to_string()).is_err());
        }
        let raw = load(directory.path(), "9203").unwrap();
        assert_eq!(raw.segments[0].text, "Original source");
        assert_eq!(render(&raw).segments[0].translation, "");
    }

    #[test]
    fn failed_save_does_not_publish_an_edit() {
        let directory = Directory::new();
        let shared = Arc::new(Mutex::new(fixture("9204")));
        let req = request(
            "9204",
            &directory,
            "caption",
            "caption-1",
            "text",
            "Unsaved",
        );
        let unavailable = directory.0.join("missing");
        assert!(
            apply_shared(
                &shared,
                &Edit::parse(&req).unwrap(),
                unavailable.to_str().unwrap()
            )
            .is_err()
        );
        assert_eq!(
            render(&shared.lock().unwrap()).captions[0].segment.text,
            "Original source"
        );
    }

    #[test]
    fn concurrent_checkpoints_retain_manual_corrections() {
        let directory = Directory::new();
        let shared = Arc::new(Mutex::new(fixture("9205")));
        let checkpoint = {
            let shared = shared.clone();
            let path = directory.path().to_owned();
            thread::spawn(move || {
                for _ in 0..20 {
                    save(&shared, &path).unwrap();
                }
            })
        };
        let req = request(
            "9205",
            &directory,
            "caption",
            "caption-1",
            "text",
            "Saved correction",
        );
        apply_shared(&shared, &Edit::parse(&req).unwrap(), directory.path()).unwrap();
        checkpoint.join().unwrap();
        let restored = render(&load(directory.path(), "9205").unwrap());
        assert_eq!(restored.captions[0].segment.text, "Saved correction");
    }

    #[test]
    fn live_and_last_commands_preserve_recording_and_audio() {
        let directory = Directory::new();
        let audio_path = directory.0.join("9206.wav");
        let wave = Arc::new(Mutex::new(Wave::new(audio_path.clone()).unwrap()));
        let original_audio = fs::read(&audio_path).unwrap();
        let mut live = fixture("9206");
        live.duration_ms = 9876;
        live.audio_path = audio_path.to_string_lossy().into();
        let shared = Arc::new(Mutex::new(live));
        let (control, _) = mpsc::channel();
        let (frames, _) = mpsc::sync_channel(1);
        let saved_engine = ENGINE.lock().unwrap().replace(Engine {
            control,
            frames,
            view: shared.clone(),
            wave,
        });
        let saved_last = std::mem::replace(&mut *LAST.lock().unwrap(), View::idle());
        let req = request(
            "9206",
            &directory,
            "caption",
            "caption-1",
            "text",
            "Live corrected",
        );
        let updated = super::super::command(&req.to_string()).unwrap();
        assert_eq!(updated["phase"], "recording");
        assert_eq!(updated["durationMs"], 9876);
        assert_eq!(snapshot().captions[0].segment.text, "Live corrected");
        // Renaming a live/paused recording must not change the phase, captions,
        // duration, or audio, nor send any recording control message.
        for phase in ["recording", "paused"] {
            shared.lock().unwrap().phase = phase.into();
            let mut expected = json!(render(&shared.lock().unwrap()));
            expected["title"] = json!(format!("Renamed while {phase}"));
            let renamed = super::super::command(&json!({"op":"rename", "id":"9206", "directory":directory.path(), "title":format!("  Renamed while {phase}  ")}).to_string()).unwrap();
            assert_eq!(renamed, expected);
            assert_eq!(snapshot().title, format!("Renamed while {phase}"));
            assert_eq!(
                load(directory.path(), "9206").unwrap().title,
                format!("Renamed while {phase}")
            );
        }
        shared.lock().unwrap().phase = "ended".into();
        let req = request(
            "9206",
            &directory,
            "caption",
            "caption-1",
            "translation",
            "Ended corrected",
        );
        super::super::command(&req.to_string()).unwrap();
        let ended_engine = ENGINE.lock().unwrap().take();
        super::super::command(&json!({"op":"rename", "id":"9206", "directory":directory.path(), "title":"Saved recording name"}).to_string()).unwrap();
        assert_eq!(snapshot().title, "Saved recording name");
        assert_eq!(
            load(directory.path(), "9206").unwrap().title,
            "Saved recording name"
        );
        assert_eq!(
            snapshot().captions[0].segment.translation,
            "Ended corrected"
        );
        let req = request(
            "9206",
            &directory,
            "caption",
            "caption-1",
            "text",
            "Last corrected",
        );
        let updated = super::super::command(&req.to_string()).unwrap();
        assert_eq!(updated["captions"][0]["text"], "Last corrected");
        assert_eq!(snapshot().captions[0].segment.text, "Last corrected");
        assert_eq!(
            render(&load(directory.path(), "9206").unwrap()).captions[0]
                .segment
                .translation,
            "Ended corrected"
        );
        assert_eq!(fs::read(&audio_path).unwrap(), original_audio);
        drop(ended_engine);
        *ENGINE.lock().unwrap() = saved_engine;
        *LAST.lock().unwrap() = saved_last;
    }
}
