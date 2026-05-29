#include "vrv_bridge.h"

namespace vrv_rs {

std::unique_ptr<vrv::Toolkit> new_toolkit(bool init_fonts) {
    return std::make_unique<vrv::Toolkit>(init_fonts);
}

rust::String get_version(const vrv::Toolkit &tk) {
    // Toolkit::GetVersion is declared `const` upstream; safe to call on a const ref.
    return rust::String(tk.GetVersion());
}

} // namespace vrv_rs
