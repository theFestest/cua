use async_trait::async_trait;
use cua_driver_core::{
    protocol::{Content, ToolResult},
    tool::{Tool, ToolDef},
};
use serde_json::Value;

pub struct OcrTool;

static DEF: std::sync::OnceLock<ToolDef> = std::sync::OnceLock::new();

fn def() -> &'static ToolDef {
    DEF.get_or_init(|| ToolDef {
        name: "ocr".into(),
        description: "Recognize on-screen text in one window with Apple Vision, on-device. \
            Returns each text line with a center in window-local screenshot pixels — the same \
            frame `get_window_state` returns and `click` consumes, so a recognized line is \
            directly clickable. Detects the script automatically, including CJK, so leave \
            `languages` unset unless you need to restrict recognition.\n\n\
            Use it when a surface has no usable Accessibility coverage: canvas, custom-drawn \
            views, games, or an image of text. The AX tree in `get_window_state` stays the \
            first choice — it carries roles, values, and actions that OCR cannot see."
            .into(),
        input_schema: serde_json::json!({
            "type": "object",
            "required": ["window_id"],
            "properties": {
                "window_id": { "type": "integer", "description": "CGWindowID from list_windows." },
                "pid":       { "type": "integer", "description": "Target pid — pass it back to click along with a returned center." },
                "languages": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional BCP-47 hints, e.g. [\"ja\"]. Omit this to let Vision detect the script, which is the better default: a hint list restricts recognition to those languages and silently drops text in any other script."
                }
            },
            "additionalProperties": false
        }),
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: false,
    })
}

#[async_trait]
impl Tool for OcrTool {
    fn def(&self) -> &ToolDef {
        def()
    }

    async fn invoke(&self, args: Value) -> ToolResult {
        use cua_driver_core::tool_args::ArgsExt;
        let window_id = match args.require_u32("window_id") {
            Ok(v) => v,
            Err(e) => return e,
        };

        let languages: Vec<String> = args
            .get("languages")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();

        let result = tokio::task::spawn_blocking(move || {
            let png_bytes = crate::capture::screenshot_window_bytes(window_id)?;
            let (w, h) = crate::capture::png_dimensions(&png_bytes)?;
            let refs: Vec<&str> = languages.iter().map(String::as_str).collect();
            let lines = cua_driver_ocr::recognize(&png_bytes, w, h, &refs)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok::<_, anyhow::Error>((lines, w, h))
        })
        .await;

        match result {
            Ok(Ok((lines, w, h))) => {
                let summary = if lines.is_empty() {
                    format!("OCR found no text in window {window_id} ({w}×{h} px).")
                } else {
                    let body = lines
                        .iter()
                        .map(|l| {
                            format!(
                                "({:.0},{:.0}) {:?}  conf={:.2}",
                                l.center.0, l.center.1, l.text, l.confidence
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    format!(
                        "OCR found {} line(s) in window {window_id} ({w}×{h} px). \
                         Centers are window-local screenshot pixels — pass one to \
                         click(pid, window_id, x, y).\n{body}",
                        lines.len()
                    )
                };
                let structured: Vec<Value> = lines
                    .iter()
                    .map(|l| {
                        serde_json::json!({
                            "text": l.text,
                            "confidence": l.confidence,
                            "center_x": l.center.0,
                            "center_y": l.center.1,
                        })
                    })
                    .collect();
                ToolResult {
                    content: vec![Content::text(summary)],
                    is_error: None,
                    structured_content: Some(serde_json::json!({
                        "lines": structured,
                        "width": w,
                        "height": h,
                        "coordinate_space": "window_screenshot_pixels",
                    })),
                }
            }
            Ok(Err(e)) => ToolResult::error(format!("OCR failed: {e}")),
            Err(e) => ToolResult::error(format!("Task error: {e}")),
        }
    }
}
