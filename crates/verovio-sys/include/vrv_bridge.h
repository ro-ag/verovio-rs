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

// Rendering surface — every method here returns an owned std::string upstream
// and Verovio offers no streaming overload, so the inevitable C++ allocation
// happens regardless. The Rust-side `_into` variants in the safe wrapper
// still eliminate the per-call `String` heap churn for the caller.
rust::String render_to_svg(vrv::Toolkit &tk, int32_t page_no, bool xml_declaration);
rust::String render_to_timemap(vrv::Toolkit &tk, rust::Str json_options);

void redo_layout(vrv::Toolkit &tk, rust::Str json_options);

rust::String get_elements_at_time(vrv::Toolkit &tk, int32_t millisec);

// Process-global log control. Verovio's log threshold lives in a
// namespace-level variable (`vrv::logLevel`); the safe-wrapper crate
// gates this behind a Mutex so concurrent toolkits don't race on it.
void enable_log(int32_t level);

} // namespace vrv_rs
