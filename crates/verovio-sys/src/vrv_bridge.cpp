#include "vrv_bridge.h"

namespace vrv_rs {

std::unique_ptr<vrv::Toolkit> new_toolkit(bool init_fonts) {
    return std::make_unique<vrv::Toolkit>(init_fonts);
}

rust::String get_version(const vrv::Toolkit &tk) {
    return rust::String(tk.GetVersion());
}

bool set_resource_path(vrv::Toolkit &tk, rust::Str path) {
    return tk.SetResourcePath(std::string(path));
}

bool load_data(vrv::Toolkit &tk, rust::Str data) {
    return tk.LoadData(std::string(data));
}

int page_count(vrv::Toolkit &tk) {
    return tk.GetPageCount();
}

rust::String get_options(const vrv::Toolkit &tk) {
    return rust::String(tk.GetOptions());
}

rust::String get_default_options(const vrv::Toolkit &tk) {
    return rust::String(tk.GetDefaultOptions());
}

bool set_options(vrv::Toolkit &tk, rust::Str options) {
    return tk.SetOptions(std::string(options));
}

} // namespace vrv_rs
