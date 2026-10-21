//! Atomic metadata updates share the same locks as subtitle edits and checkpoints.
use super::*;

pub(super) fn update_shared(
    view: &Arc<Mutex<View>>,
    directory: &str,
    change: &mut impl FnMut(&mut View) -> Result<(), String>,
) -> Result<View, String> {
    let _writer = FILE_WRITE_LOCK.lock().map_err(|_| "记录保存状态异常")?;
    let mut raw = view.lock().map_err(|_| "字幕状态读取失败")?;
    let mut updated = raw.clone();
    change(&mut updated)?;
    write_record(&updated, directory)?;
    *raw = updated.clone();
    if raw.phase == "ended" {
        if let Ok(mut last) = LAST.lock() {
            *last = updated.clone();
        }
    }
    Ok(updated)
}

// Network requests never hold the recording, checkpoint, or file locks.
pub(super) fn update(
    req: &Value,
    mut change: impl FnMut(&mut View) -> Result<(), String>,
) -> Result<View, String> {
    let id = req["id"].as_str().ok_or("缺少记录 ID")?;
    if !valid_id(id) {
        return Err("无效记录 ID".into());
    }
    let directory = req["directory"].as_str().ok_or("缺少目录")?;
    let engine = ENGINE.lock().map_err(|_| "状态异常")?;
    if let Some(engine) = engine.as_ref() {
        if engine.view.lock().map_err(|_| "字幕状态读取失败")?.id == id {
            return update_shared(&engine.view, directory, &mut change);
        }
    }
    drop(engine);
    let _writer = FILE_WRITE_LOCK.lock().map_err(|_| "记录保存状态异常")?;
    let mut last = LAST.lock().map_err(|_| "字幕状态读取失败")?;
    if last.id == id {
        let mut updated = last.clone();
        change(&mut updated)?;
        write_record(&updated, directory)?;
        *last = updated.clone();
        return Ok(updated);
    }
    drop(last);
    let mut updated = load(directory, id)?;
    change(&mut updated)?;
    write_record(&updated, directory)?;
    Ok(updated)
}

pub(super) fn rename(req: &Value) -> Result<Value, String> {
    let title = req["title"].as_str().ok_or("记录名称不能为空")?.trim();
    if title.is_empty() {
        return Err("记录名称不能为空".into());
    }
    let updated = update(req, |v| {
        v.title = title.to_owned();
        Ok(())
    })?;
    Ok(json!(caption_edits::render(&updated)))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "lingodesk-names-{}-{}",
                std::process::id(),
                text_translation::new_key()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn req(&self, title: &str) -> Value {
            json!({"op":"rename", "id":"9401", "directory":self.0.to_str().unwrap(), "title":title})
        }
        fn fixture(&self) -> View {
            let view = View {
                id: "9401".into(),
                title: "Before".into(),
                phase: "ended".into(),
                duration_ms: 1234,
                segments: vec![Segment {
                    id: "first".into(),
                    text: "Original".into(),
                    translation: "译文".into(),
                    ..Segment::default()
                }],
                ..View::idle()
            };
            fs::write(self.0.join("9401.json"), serde_json::to_vec(&view).unwrap()).unwrap();
            view
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn historical_rename_survives_reload_list_and_exports_without_modifying_recording_content() {
        let dir = Directory::new();
        let original = dir.fixture();
        let request = dir.req("  会議 ✨ Meeting  ");
        let mut expected = json!(original);
        expected["title"] = json!("会議 ✨ Meeting");
        assert_eq!(
            super::super::command(&request.to_string()).unwrap(),
            expected
        );
        let mut request = dir.req("");
        request["op"] = json!("load");
        assert_eq!(
            super::super::command(&request.to_string()).unwrap(),
            expected
        );
        request["op"] = json!("list");
        let listed = super::super::command(&request.to_string()).unwrap();
        assert_eq!(listed[0], expected);
        for format in ["txt", "json"] {
            request["op"] = json!("export");
            request["format"] = json!(format);
            let output = super::super::command(&request.to_string()).unwrap();
            assert!(
                output["content"]
                    .as_str()
                    .unwrap()
                    .contains("会議 ✨ Meeting")
            );
            assert!(output["content"].as_str().unwrap().contains("Original"));
        }
    }

    #[test]
    fn invalid_names_and_ids_preserve_the_saved_record() {
        let dir = Directory::new();
        let original = dir.fixture();
        for title in ["", "  ", "\n\t"] {
            assert_eq!(rename(&dir.req(title)).unwrap_err(), "记录名称不能为空");
        }
        for id in ["", "../9401", "missing", "9402"] {
            let mut request = dir.req("Wrong record");
            request["id"] = json!(id);
            assert!(rename(&request).is_err());
        }
        let restored = load(dir.0.to_str().unwrap(), "9401").unwrap();
        assert_eq!(json!(restored), json!(original));
    }
}
