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

        // LoadData and SetOptions return bool: true on success, false on
        // failure (Verovio logs the reason internally; the log surface is
        // gated by a crate-level mutex in the safe wrapper).
        fn load_data(tk: Pin<&mut Toolkit>, data: &str) -> bool;
        fn set_options(tk: Pin<&mut Toolkit>, options: &str) -> bool;

        // Toolkit::GetPageCount is non-const upstream — Verovio computes
        // layout lazily on this call. Surface it as `Pin<&mut>` accordingly.
        fn page_count(tk: Pin<&mut Toolkit>) -> i32;

        // Const-correct getters.
        fn get_options(tk: &Toolkit) -> String;
        fn get_default_options(tk: &Toolkit) -> String;

        // Rendering surface. All are non-const upstream — Verovio computes
        // layout lazily and stores it on the Toolkit instance.
        fn render_to_svg(tk: Pin<&mut Toolkit>, page_no: i32, xml_declaration: bool) -> String;
        fn render_to_timemap(tk: Pin<&mut Toolkit>, json_options: &str) -> String;
        fn redo_layout(tk: Pin<&mut Toolkit>, json_options: &str);
        fn get_elements_at_time(tk: Pin<&mut Toolkit>, millisec: i32) -> String;
    }
}
