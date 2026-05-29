#pragma once

#include <memory>
#include "rust/cxx.h"
#include "vrv/toolkit.h"

namespace vrv_rs {

std::unique_ptr<vrv::Toolkit> new_toolkit(bool init_fonts);

rust::String get_version(const vrv::Toolkit &tk);

} // namespace vrv_rs
