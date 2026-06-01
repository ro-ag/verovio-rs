//! Low-level [`cxx`] bridge to the [Verovio](https://www.verovio.org/) C++
//! music engraving library.
//!
//! This crate compiles the vendored Verovio source tree into a static
//! archive and exposes the `Toolkit` class through a thin C++ shim. You
//! almost certainly want the safe wrapper in the [`verovio`](https://docs.rs/verovio)
//! crate instead — this crate is a build dependency, not a user-facing API.

#[cxx::bridge(namespace = "vrv_rs")]
pub mod ffi {
    unsafe extern "C++" {
        include!("vrv_bridge.h");

        #[namespace = "vrv"]
        type Toolkit;

        fn new_toolkit(init_fonts: bool) -> UniquePtr<Toolkit>;

        fn get_version(tk: &Toolkit) -> String;

        /// Point Verovio at a directory containing SMuFL font data
        /// (Bravura.xml, Bravura/, etc.). Returns true on success. Must
        /// succeed before any LoadData / RenderTo* call.
        fn set_resource_path(tk: Pin<&mut Toolkit>, path: &str) -> bool;
        fn get_resource_path(tk: &Toolkit) -> String;

        /// Upstream `Doc::GetID` — opaque per-document identifier; useful
        /// for diagnostics / log correlation. Non-`const` upstream.
        fn get_id(tk: Pin<&mut Toolkit>) -> String;

        // LoadData and SetOptions return bool: true on success, false on
        // failure (Verovio logs the reason internally; the log surface is
        // gated by a crate-level mutex in the safe wrapper).
        fn load_data(tk: Pin<&mut Toolkit>, data: &str) -> bool;
        /// Delegate to `Toolkit::LoadFile` — handles UTF-16 MusicXML and
        /// compressed `.mxl` transparently. Use when you want the full
        /// upstream loader rather than reading into a `String` first.
        fn load_file(tk: Pin<&mut Toolkit>, filename: &str) -> bool;
        /// Load a compressed MusicXML file (`.mxl`) from raw bytes.
        fn load_zip_data_buffer(tk: Pin<&mut Toolkit>, data: &[u8]) -> bool;

        fn set_options(tk: Pin<&mut Toolkit>, options: &str) -> bool;
        fn reset_options(tk: Pin<&mut Toolkit>);
        /// Apply a scoped selection (measure/staff range). See
        /// `Toolkit::Select` upstream for the JSON schema. An empty
        /// string / empty object clears the selection.
        fn select(tk: Pin<&mut Toolkit>, selection: &str) -> bool;

        // Toolkit::GetPageCount is non-const upstream — Verovio computes
        // layout lazily on this call. Surface it as `Pin<&mut>` accordingly.
        fn page_count(tk: Pin<&mut Toolkit>) -> i32;

        // Const-correct getters.
        fn get_options(tk: &Toolkit) -> String;
        fn get_default_options(tk: &Toolkit) -> String;
        /// Full option schema (type, default, min, max, grouped by
        /// category). Const upstream.
        fn get_available_options(tk: &Toolkit) -> String;

        /// Typed scale setter — avoids round-tripping `{"scale": N}` JSON.
        fn set_scale(tk: Pin<&mut Toolkit>, scale: i32) -> bool;
        fn get_scale(tk: Pin<&mut Toolkit>) -> i32;

        /// Force the input format instead of auto-detect. Accepts the same
        /// strings Verovio's `--from` CLI flag does (`"mei"`, `"musicxml"`,
        /// `"abc"`, `"pae"`, `"humdrum"`, …).
        fn set_input_from(tk: Pin<&mut Toolkit>, format: &str) -> bool;
        fn set_output_to(tk: Pin<&mut Toolkit>, format: &str) -> bool;

        /// Re-seed the `@xml:id` generator. Pass `0` for a time-based
        /// random seed. No-op when `--xml-id-checksum` is set.
        fn reset_xml_id_seed(tk: Pin<&mut Toolkit>, seed: i32);

        // Rendering surface. All are non-const upstream — Verovio computes
        // layout lazily and stores it on the Toolkit instance.
        fn render_to_svg(tk: Pin<&mut Toolkit>, page_no: i32, xml_declaration: bool) -> String;
        fn render_to_midi(tk: Pin<&mut Toolkit>) -> String;
        fn render_to_timemap(tk: Pin<&mut Toolkit>, json_options: &str) -> String;
        fn render_to_expansion_map(tk: Pin<&mut Toolkit>) -> String;
        /// Render the document as Plaine & Easie code. Only the top staff
        /// / layer is exported (upstream limitation).
        fn render_to_pae(tk: Pin<&mut Toolkit>) -> String;

        /// Validate Plaine & Easie input — returns a JSON object with
        /// warnings / errors. **Discards any previously loaded data.**
        fn validate_pae(tk: Pin<&mut Toolkit>, data: &str) -> String;

        /// Serialize the loaded document as MEI. `json_options` is upstream's
        /// `{pageNo, scoreBased, basic, removeIds}` JSON document; pass an
        /// empty string for defaults.
        fn get_mei(tk: Pin<&mut Toolkit>, json_options: &str) -> String;

        /// Extract descriptive features tailored for incipit search.
        /// `json_options` is upstream's feature-extraction option schema.
        fn get_descriptive_features(tk: Pin<&mut Toolkit>, json_options: &str) -> String;

        fn redo_layout(tk: Pin<&mut Toolkit>, json_options: &str);
        /// Cheaper than `redo_layout` — recalculates only note vertical
        /// positions on the current page. Use after option changes that
        /// don't affect horizontal spacing.
        fn redo_page_pitch_pos_layout(tk: Pin<&mut Toolkit>);

        fn get_elements_at_time(tk: Pin<&mut Toolkit>, millisec: i32) -> String;

        // Element introspection — inverse of get_elements_at_time, plus
        // attribute readback for the hit-testing/click-to-seek pathway.
        /// Page (1-based) on which `xml_id` is rendered, or `0` if missing.
        fn get_page_with_element(tk: Pin<&mut Toolkit>, xml_id: &str) -> i32;
        /// Wall-clock millisecond onset of `xml_id`. `RenderToMIDI` (or
        /// `render_to_timemap`) must have run first to populate the time
        /// table; the safe wrapper handles this transparently.
        fn get_time_for_element(tk: Pin<&mut Toolkit>, xml_id: &str) -> i32;
        /// JSON object with MIDI pitch, duration, channel, etc. for the
        /// element. Empty / `"{}"` if `xml_id` is unknown or not a note.
        fn get_midi_values_for_element(tk: Pin<&mut Toolkit>, xml_id: &str) -> String;
        /// JSON object with score-time + real-time onset/offset/tied
        /// duration. Same prerequisite as `get_time_for_element`.
        fn get_times_for_element(tk: Pin<&mut Toolkit>, xml_id: &str) -> String;
        /// JSON object of every MEI attribute on `xml_id` (including
        /// attributes Verovio itself doesn't honor).
        fn get_element_attr(tk: Pin<&mut Toolkit>, xml_id: &str) -> String;
        /// MEI ID of the **notated** element when the input id refers to
        /// an expansion clone. Returns `xml_id` itself if no expansion.
        fn get_notated_id_for_element(tk: Pin<&mut Toolkit>, xml_id: &str) -> String;
        /// JSON array of every expansion clone id sharing a notated id.
        fn get_expansion_ids_for_element(tk: Pin<&mut Toolkit>, xml_id: &str) -> String;

        // Process-global log threshold. Gated behind a Mutex in the safe
        // wrapper so concurrent toolkit users don't race on Verovio's
        // namespace-level `vrv::logLevel`.
        fn enable_log(level: i32);
    }
}
