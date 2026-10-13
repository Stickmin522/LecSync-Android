//! Manual corrections are an overlay: matching continues to use the original stream.
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
            fields.text = Some(self.value.to_owned());
        } else {
            fields.translation = Some(self.value.to_owned());
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

fn apply_shared(view: &Arc<Mutex<View>>, edit: &Edit<'_>, directory: &str) -> Result<View, String> {
    let _writer = FILE_WRITE_LOCK.lock().map_err(|_| "记录保存状态异常")?;
    let mut raw = view.lock().map_err(|_| "字幕状态读取失败")?;
    let mut updated = raw.clone();
    edit.apply(&mut updated)?;
    write_record(&updated, directory)?;
    // Publish only once persistence succeeds; a failed save leaves both views unchanged.
    *raw = updated;
    if raw.phase == "ended" {
        if let Ok(mut last) = LAST.lock() {
            *last = raw.clone();
        }
    }
    Ok(render(&raw))
}

pub(super) fn command(req: &Value) -> Result<Value, String> {
    let id = req["id"].as_str().ok_or("缺少记录 ID")?;
    if !valid_id(id) {
        return Err("无效记录 ID".into());
    }
    let directory = req["directory"].as_str().ok_or("缺少目录")?;
    let edit = Edit::parse(req)?;
    let engine = ENGINE.lock().map_err(|_| "状态异常")?;
    if let Some(engine) = engine.as_ref() {
        if engine.view.lock().map_err(|_| "字幕状态读取失败")?.id == id {
            return Ok(json!(apply_shared(&engine.view, &edit, directory)?));
        }
    }
    drop(engine);

    let _writer = FILE_WRITE_LOCK.lock().map_err(|_| "记录保存状态异常")?;
    let mut last = LAST.lock().map_err(|_| "字幕状态读取失败")?;
    if last.id == id {
        let mut updated = last.clone();
        edit.apply(&mut updated)?;
        write_record(&updated, directory)?;
        *last = updated;
        return Ok(json!(render(&last)));
    }
    drop(last);
    let mut updated = load(directory, id)?;
    edit.apply(&mut updated)?;
    write_record(&updated, directory)?;
    Ok(json!(render(&updated)))
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
