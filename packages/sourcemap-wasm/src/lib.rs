use srcmap_sourcemap::{Bias, GeneratedLocation, OriginalLocation};
use std::ptr::addr_of;
use wasm_bindgen::prelude::*;

/// Static result buffer for zero-allocation single lookups.
/// Layout: [sourceIdx, line, column, nameIdx]. Values of -1 indicate no mapping/no name.
static mut RESULT_BUF: [i32; 4] = [-1, -1, -1, -1];

/// Get the pointer to the static result buffer in WASM linear memory.
/// JS side creates an Int32Array view at this offset to read lookup results
/// without any allocation or copying.
#[wasm_bindgen(js_name = "resultPtr")]
pub fn result_ptr() -> *const i32 {
    // Use addr_of! to avoid creating a reference to static mut (Rust 2024)
    addr_of!(RESULT_BUF).cast::<i32>()
}

/// Expose WASM linear memory for direct buffer access from JS.
#[wasm_bindgen(js_name = "wasmMemory")]
pub fn wasm_memory() -> JsValue {
    wasm_bindgen::memory()
}

fn bias_from(bias: i32) -> Bias {
    if bias == -1 { Bias::LeastUpperBound } else { Bias::GreatestLowerBound }
}

/// Builds the `{source, line, column, name}` object returned by the lookup methods.
fn original_position_object(
    loc: &OriginalLocation,
    source: Option<&str>,
    name: Option<&str>,
) -> JsValue {
    let obj = js_sys::Object::new();
    let source: JsValue = source.map_or(JsValue::NULL, |s| s.into());
    js_sys::Reflect::set(&obj, &"source".into(), &source).unwrap_or(false);
    js_sys::Reflect::set(&obj, &"line".into(), &loc.line.into()).unwrap_or(false);
    js_sys::Reflect::set(&obj, &"column".into(), &loc.column.into()).unwrap_or(false);
    let name: JsValue = name.map_or(JsValue::NULL, |s| s.into());
    js_sys::Reflect::set(&obj, &"name".into(), &name).unwrap_or(false);
    obj.into()
}

fn generated_position_object(loc: &GeneratedLocation) -> JsValue {
    let obj = js_sys::Object::new();
    js_sys::Reflect::set(&obj, &"line".into(), &loc.line.into()).unwrap_or(false);
    js_sys::Reflect::set(&obj, &"column".into(), &loc.column.into()).unwrap_or(false);
    obj.into()
}

/// `[sourceIdx, line, column, nameIdx]`, with -1 for a missing mapping or name.
fn flat_position(loc: Option<&OriginalLocation>) -> [i32; 4] {
    loc.map_or([-1; 4], |loc| {
        [loc.source as i32, loc.line as i32, loc.column as i32, loc.name.map_or(-1, |n| n as i32)]
    })
}

/// Batch lookup over a flat `[line0, col0, line1, col1, ...]` array; a trailing odd value is ignored.
fn flat_positions_for(
    positions: &[i32],
    lookup: impl Fn(u32, u32) -> Option<OriginalLocation>,
) -> Vec<i32> {
    let mut out = Vec::with_capacity(positions.len() / 2 * 4);
    for pair in positions.as_chunks::<2>().0 {
        out.extend_from_slice(&flat_position(lookup(pair[0] as u32, pair[1] as u32).as_ref()));
    }
    out
}

#[wasm_bindgen]
pub struct SourceMap {
    inner: srcmap_sourcemap::SourceMap,
}

#[wasm_bindgen]
impl SourceMap {
    /// Parse a source map from a JSON string (full parse including sourcesContent).
    #[wasm_bindgen(constructor)]
    pub fn new(json: &str) -> Result<SourceMap, JsError> {
        let inner = srcmap_sourcemap::SourceMap::from_json(json)
            .map_err(|e| JsError::new(&e.to_string()))?;
        Ok(Self { inner })
    }

    /// Parse a source map from a `data:` URL (base64 or plain JSON).
    #[wasm_bindgen(js_name = "fromDataUrl")]
    pub fn from_data_url(url: &str) -> Result<SourceMap, JsError> {
        let inner = srcmap_sourcemap::SourceMap::from_data_url(url)
            .map_err(|e| JsError::new(&e.to_string()))?;
        Ok(Self { inner })
    }

    /// Parse a source map from JSON, skipping sourcesContent allocation.
    /// Used by the JS wrapper which keeps sourcesContent on the JS side.
    #[wasm_bindgen(js_name = "fromJsonNoContent")]
    pub fn from_json_no_content(json: &str) -> Result<SourceMap, JsError> {
        let inner = srcmap_sourcemap::SourceMap::from_json_no_content(json)
            .map_err(|e| JsError::new(&e.to_string()))?;
        Ok(Self { inner })
    }

    /// Build a source map from pre-parsed components (fast path).
    ///
    /// JS does JSON.parse() (V8-native speed), then only the VLQ mappings string
    /// is sent to WASM for decoding. sourcesContent is NOT copied — keep it JS-side.
    #[wasm_bindgen(js_name = "fromVlq")]
    #[allow(
        clippy::too_many_arguments,
        reason = "JS passes parsed source map parts separately to avoid copying sourcesContent"
    )]
    pub fn from_vlq(
        mappings: &str,
        sources: Vec<JsValue>,
        names: Vec<JsValue>,
        file: Option<String>,
        source_root: Option<String>,
        ignore_list: Vec<u32>,
        debug_id: Option<String>,
    ) -> Result<SourceMap, JsError> {
        let sources: Vec<String> =
            sources.iter().map(|s| s.as_string().unwrap_or_default()).collect();
        let names: Vec<String> = names.iter().map(|s| s.as_string().unwrap_or_default()).collect();

        let inner = srcmap_sourcemap::SourceMap::from_vlq(
            mappings,
            sources,
            names,
            file,
            source_root,
            Vec::new(), // sourcesContent kept JS-side
            ignore_list,
            debug_id,
        )
        .map_err(|e| JsError::new(&e.to_string()))?;

        Ok(Self { inner })
    }

    /// Look up the original source position for a generated position.
    /// Both line and column are 0-based.
    /// Returns null if no mapping exists, or an object {source, line, column, name}.
    #[wasm_bindgen(js_name = "originalPositionFor")]
    pub fn original_position_for(&self, line: u32, column: u32) -> JsValue {
        self.original_position_for_with_bias(line, column, 0)
    }

    /// Look up the original source position with a bias.
    /// bias: 0 = GREATEST_LOWER_BOUND (default), -1 = LEAST_UPPER_BOUND
    #[wasm_bindgen(js_name = "originalPositionForWithBias")]
    pub fn original_position_for_with_bias(&self, line: u32, column: u32, bias: i32) -> JsValue {
        self.inner.original_position_for_with_bias(line, column, bias_from(bias)).map_or(
            JsValue::NULL,
            |loc| {
                original_position_object(
                    &loc,
                    self.inner.get_source(loc.source),
                    loc.name.and_then(|i| self.inner.get_name(i)),
                )
            },
        )
    }

    /// Fast single lookup returning flat array [sourceIdx, line, column, nameIdx].
    #[wasm_bindgen(js_name = "originalPositionFlat")]
    pub fn original_position_flat(&self, line: u32, column: u32) -> Vec<i32> {
        flat_position(self.inner.original_position_for(line, column).as_ref()).to_vec()
    }

    /// Zero-allocation single lookup via static buffer.
    #[wasm_bindgen(js_name = "originalPositionBuf")]
    pub fn original_position_buf(&self, line: u32, column: u32) -> bool {
        match self.inner.original_position_for(line, column) {
            Some(loc) => {
                // SAFETY: RESULT_BUF is a four-slot static buffer in single-threaded WASM,
                // and this write stays entirely within that fixed allocation.
                unsafe {
                    let buf = std::ptr::addr_of_mut!(RESULT_BUF);
                    (*buf)[0] = loc.source as i32;
                    (*buf)[1] = loc.line as i32;
                    (*buf)[2] = loc.column as i32;
                    (*buf)[3] = loc.name.map_or(-1, |n| n as i32);
                }
                true
            }
            None => false,
        }
    }

    #[wasm_bindgen(js_name = "generatedPositionFor")]
    pub fn generated_position_for(&self, source: &str, line: u32, column: u32) -> JsValue {
        self.generated_position_for_with_bias(source, line, column, 0)
    }

    /// Look up the generated position with a bias.
    /// bias: 0 = GREATEST_LOWER_BOUND (default), -1 = LEAST_UPPER_BOUND
    /// (same convention as originalPositionForWithBias)
    #[wasm_bindgen(js_name = "generatedPositionForWithBias")]
    pub fn generated_position_for_with_bias(
        &self,
        source: &str,
        line: u32,
        column: u32,
        bias: i32,
    ) -> JsValue {
        self.inner
            .generated_position_for_with_bias(source, line, column, bias_from(bias))
            .map_or(JsValue::NULL, |loc| generated_position_object(&loc))
    }

    #[wasm_bindgen(js_name = "mapRange")]
    pub fn map_range(
        &self,
        start_line: u32,
        start_column: u32,
        end_line: u32,
        end_column: u32,
    ) -> JsValue {
        match self.inner.map_range(start_line, start_column, end_line, end_column) {
            Some(range) => {
                let obj = js_sys::Object::new();
                let source: JsValue =
                    self.inner.get_source(range.source).map_or(JsValue::NULL, |s| s.into());
                js_sys::Reflect::set(&obj, &"source".into(), &source).unwrap_or(false);
                js_sys::Reflect::set(
                    &obj,
                    &"originalStartLine".into(),
                    &range.original_start_line.into(),
                )
                .unwrap_or(false);
                js_sys::Reflect::set(
                    &obj,
                    &"originalStartColumn".into(),
                    &range.original_start_column.into(),
                )
                .unwrap_or(false);
                js_sys::Reflect::set(
                    &obj,
                    &"originalEndLine".into(),
                    &range.original_end_line.into(),
                )
                .unwrap_or(false);
                js_sys::Reflect::set(
                    &obj,
                    &"originalEndColumn".into(),
                    &range.original_end_column.into(),
                )
                .unwrap_or(false);
                obj.into()
            }
            None => JsValue::NULL,
        }
    }

    #[wasm_bindgen(js_name = "allGeneratedPositionsFor")]
    pub fn all_generated_positions_for(
        &self,
        source: &str,
        line: u32,
        column: u32,
    ) -> Vec<JsValue> {
        self.inner
            .all_generated_positions_for(source, line, column)
            .iter()
            .map(generated_position_object)
            .collect()
    }

    /// Returns the source filename at the given index, or `None` if the index is out of bounds.
    #[wasm_bindgen]
    pub fn source(&self, index: u32) -> Option<String> {
        self.inner.get_source(index).map(|s| s.to_string())
    }

    /// Returns the name at the given index, or `None` if the index is out of bounds.
    #[wasm_bindgen]
    pub fn name(&self, index: u32) -> Option<String> {
        self.inner.get_name(index).map(|s| s.to_string())
    }

    #[wasm_bindgen(getter)]
    pub fn file(&self) -> Option<String> {
        self.inner.file.clone()
    }

    #[wasm_bindgen(getter, js_name = "sourceRoot")]
    pub fn source_root(&self) -> Option<String> {
        self.inner.source_root.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn sources(&self) -> Vec<JsValue> {
        self.inner.sources.iter().map(|s| JsValue::from_str(s)).collect()
    }

    #[wasm_bindgen(getter)]
    pub fn names(&self) -> Vec<JsValue> {
        self.inner.names.iter().map(|s| JsValue::from_str(s)).collect()
    }

    #[wasm_bindgen(getter, js_name = "sourcesContent")]
    pub fn sources_content(&self) -> Vec<JsValue> {
        self.inner
            .sources_content
            .iter()
            .map(|c| match c {
                Some(s) => JsValue::from_str(s),
                None => JsValue::NULL,
            })
            .collect()
    }

    #[wasm_bindgen(getter, js_name = "ignoreList")]
    pub fn ignore_list(&self) -> Vec<u32> {
        self.inner.ignore_list.clone()
    }

    #[wasm_bindgen(js_name = "sourceContentFor")]
    pub fn source_content_for(&self, index: u32) -> JsValue {
        match self.inner.sources_content.get(index as usize) {
            Some(Some(content)) => JsValue::from_str(content),
            _ => JsValue::NULL,
        }
    }

    #[wasm_bindgen(js_name = "isIgnoredIndex")]
    pub fn is_ignored_index(&self, index: u32) -> bool {
        self.inner.ignore_list.contains(&index)
    }

    #[wasm_bindgen(getter, js_name = "debugId")]
    pub fn debug_id(&self) -> Option<String> {
        self.inner.debug_id.clone()
    }

    #[wasm_bindgen(getter, js_name = "mappingCount")]
    pub fn mapping_count(&self) -> u32 {
        self.inner.mapping_count() as u32
    }

    #[wasm_bindgen(getter, js_name = "lineCount")]
    pub fn line_count(&self) -> u32 {
        self.inner.line_count() as u32
    }

    /// Serialize the source map to a JSON string.
    #[wasm_bindgen(js_name = "toJSON")]
    pub fn to_json(&self) -> String {
        self.inner.to_json()
    }

    /// Serialize the source map to a `data:` URL (base64-encoded JSON).
    #[wasm_bindgen(js_name = "toDataUrl")]
    pub fn to_data_url(&self) -> String {
        self.inner.to_data_url()
    }

    /// Set or clear the `file` property.
    #[wasm_bindgen(setter, js_name = "file")]
    pub fn set_file(&mut self, file: Option<String>) {
        self.inner.set_file(file);
    }

    /// Set or clear the `debugId` property.
    #[wasm_bindgen(setter, js_name = "debugId")]
    pub fn set_debug_id(&mut self, debug_id: Option<String>) {
        self.inner.set_debug_id(debug_id);
    }

    /// Set or clear the `sourceRoot` property.
    #[wasm_bindgen(setter, js_name = "sourceRoot")]
    pub fn set_source_root(&mut self, source_root: Option<String>) {
        self.inner.set_source_root(source_root);
    }

    #[wasm_bindgen(js_name = "encodedMappings")]
    pub fn encoded_mappings(&self) -> String {
        self.inner.encode_mappings()
    }

    #[wasm_bindgen(getter, js_name = "hasRangeMappings")]
    pub fn has_range_mappings(&self) -> bool {
        self.inner.has_range_mappings()
    }

    #[wasm_bindgen(getter, js_name = "rangeMappingCount")]
    pub fn range_mapping_count(&self) -> u32 {
        self.inner.range_mapping_count() as u32
    }

    #[wasm_bindgen(js_name = "encodedRangeMappings")]
    pub fn encoded_range_mappings(&self) -> JsValue {
        match self.inner.encode_range_mappings() {
            Some(s) => JsValue::from_str(&s),
            None => JsValue::NULL,
        }
    }

    #[wasm_bindgen(js_name = "allMappingsFlat")]
    pub fn all_mappings_flat(&self) -> Vec<i32> {
        let mappings = self.inner.all_mappings();
        let mut out = Vec::with_capacity(mappings.len() * 7);
        for m in mappings {
            out.push(m.generated_line as i32);
            out.push(m.generated_column as i32);
            out.push(if m.source == u32::MAX { -1 } else { m.source as i32 });
            out.push(m.original_line as i32);
            out.push(m.original_column as i32);
            out.push(if m.name == u32::MAX { -1 } else { m.name as i32 });
            out.push(i32::from(m.is_range_mapping));
        }
        out
    }

    #[wasm_bindgen(js_name = "originalPositionsFor")]
    pub fn original_positions_for(&self, positions: &[i32]) -> Vec<i32> {
        flat_positions_for(positions, |line, column| self.inner.original_position_for(line, column))
    }
}

/// Lazy source map that defers VLQ decoding until lookup time.
/// Parse is fast (JSON + prescan only), VLQ decode happens per-line on demand.
#[wasm_bindgen(js_name = "LazySourceMap")]
pub struct LazySourceMap {
    inner: srcmap_sourcemap::LazySourceMap,
}

#[wasm_bindgen(js_class = "LazySourceMap")]
impl LazySourceMap {
    /// Parse a source map from JSON using fast-scan mode.
    /// Only scans for semicolons (no VLQ decode). sourcesContent skipped.
    #[wasm_bindgen(constructor)]
    pub fn new(json: &str) -> Result<LazySourceMap, JsError> {
        let inner = srcmap_sourcemap::LazySourceMap::from_json_fast(json)
            .map_err(|e| JsError::new(&e.to_string()))?;
        Ok(Self { inner })
    }

    /// Build from pre-parsed components. JS sends mappings + metadata JSON (no sourcesContent).
    /// Only 2 strings cross the boundary — no per-element `Vec<JsValue>` overhead.
    #[wasm_bindgen(js_name = "fromParts")]
    pub fn from_parts(mappings: &str, metadata_json: &str) -> Result<LazySourceMap, JsError> {
        let raw: srcmap_sourcemap::RawSourceMapLite<'_> =
            serde_json::from_str(metadata_json).map_err(|e| JsError::new(&e.to_string()))?;

        let source_root = raw.source_root.as_deref().unwrap_or("");
        let sources = srcmap_sourcemap::resolve_sources(&raw.sources, source_root);
        let names = raw.names;

        let ignore_list = match raw.ignore_list {
            Some(list) => list,
            None => raw.x_google_ignore_list.unwrap_or_default(),
        };

        let inner = srcmap_sourcemap::LazySourceMap::from_vlq(
            mappings,
            sources,
            names,
            raw.file,
            raw.source_root,
            ignore_list,
            raw.debug_id,
        )
        .map_err(|e| JsError::new(&e.to_string()))?;

        Ok(Self { inner })
    }

    #[wasm_bindgen(js_name = "originalPositionFor")]
    pub fn original_position_for(&self, line: u32, column: u32) -> JsValue {
        self.inner.original_position_for(line, column).map_or(JsValue::NULL, |loc| {
            original_position_object(
                &loc,
                self.inner.get_source(loc.source),
                loc.name.and_then(|i| self.inner.get_name(i)),
            )
        })
    }

    #[wasm_bindgen(js_name = "originalPositionFlat")]
    pub fn original_position_flat(&self, line: u32, column: u32) -> Vec<i32> {
        flat_position(self.inner.original_position_for(line, column).as_ref()).to_vec()
    }

    #[wasm_bindgen(js_name = "originalPositionBuf")]
    pub fn original_position_buf(&self, line: u32, column: u32) -> bool {
        match self.inner.original_position_for(line, column) {
            Some(loc) => {
                // SAFETY: RESULT_BUF is a four-slot static buffer in single-threaded WASM,
                // and this write stays entirely within that fixed allocation.
                unsafe {
                    let buf = std::ptr::addr_of_mut!(RESULT_BUF);
                    (*buf)[0] = loc.source as i32;
                    (*buf)[1] = loc.line as i32;
                    (*buf)[2] = loc.column as i32;
                    (*buf)[3] = loc.name.map_or(-1, |n| n as i32);
                }
                true
            }
            None => false,
        }
    }

    #[wasm_bindgen(js_name = "originalPositionsFor")]
    pub fn original_positions_for(&self, positions: &[i32]) -> Vec<i32> {
        flat_positions_for(positions, |line, column| self.inner.original_position_for(line, column))
    }

    /// Returns the source filename at the given index, or `None` if the index is out of bounds.
    #[wasm_bindgen]
    pub fn source(&self, index: u32) -> Option<String> {
        self.inner.get_source(index).map(|s| s.to_string())
    }

    /// Returns the name at the given index, or `None` if the index is out of bounds.
    #[wasm_bindgen]
    pub fn name(&self, index: u32) -> Option<String> {
        self.inner.get_name(index).map(|s| s.to_string())
    }

    #[wasm_bindgen(getter)]
    pub fn file(&self) -> Option<String> {
        self.inner.file.clone()
    }

    #[wasm_bindgen(getter, js_name = "sourceRoot")]
    pub fn source_root(&self) -> Option<String> {
        self.inner.source_root.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn sources(&self) -> Vec<JsValue> {
        self.inner.sources.iter().map(|s| JsValue::from_str(s)).collect()
    }

    #[wasm_bindgen(getter)]
    pub fn names(&self) -> Vec<JsValue> {
        self.inner.names.iter().map(|s| JsValue::from_str(s)).collect()
    }

    #[wasm_bindgen(getter, js_name = "ignoreList")]
    pub fn ignore_list(&self) -> Vec<u32> {
        self.inner.ignore_list.clone()
    }

    #[wasm_bindgen(js_name = "isIgnoredIndex")]
    pub fn is_ignored_index(&self, index: u32) -> bool {
        self.inner.ignore_list.contains(&index)
    }

    #[wasm_bindgen(getter, js_name = "debugId")]
    pub fn debug_id(&self) -> Option<String> {
        self.inner.debug_id.clone()
    }

    #[wasm_bindgen(getter, js_name = "lineCount")]
    pub fn line_count(&self) -> u32 {
        self.inner.line_count() as u32
    }
}
