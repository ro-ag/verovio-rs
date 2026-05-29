#[cxx::bridge(namespace = "vrv_rs")]
pub mod ffi {
    unsafe extern "C++" {
        include!("vrv_bridge.h");

        #[namespace = "vrv"]
        type Toolkit;

        fn new_toolkit(init_fonts: bool) -> UniquePtr<Toolkit>;

        fn get_version(tk: &Toolkit) -> String;

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
    }
}
