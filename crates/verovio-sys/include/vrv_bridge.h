#pragma once

#include <memory>
#include "rust/cxx.h"
#include "vrv/toolkit.h"

namespace vrv_rs {

std::unique_ptr<vrv::Toolkit> new_toolkit(bool init_fonts);

rust::String get_version(const vrv::Toolkit &tk);

bool set_resource_path(vrv::Toolkit &tk, rust::Str path);

bool load_data(vrv::Toolkit &tk, rust::Str data);

int page_count(vrv::Toolkit &tk);

rust::String get_options(const vrv::Toolkit &tk);
rust::String get_default_options(const vrv::Toolkit &tk);
bool set_options(vrv::Toolkit &tk, rust::Str options);

} // namespace vrv_rs
