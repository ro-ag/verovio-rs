#[cxx::bridge(namespace = "vrv_rs")]
pub mod ffi {
    unsafe extern "C++" {
        include!("vrv_bridge.h");

        #[namespace = "vrv"]
        type Toolkit;

        fn new_toolkit(init_fonts: bool) -> UniquePtr<Toolkit>;
        fn get_version(tk: &Toolkit) -> String;
    }
}
