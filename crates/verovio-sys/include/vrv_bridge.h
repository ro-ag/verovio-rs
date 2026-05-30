#pragma once

#include <memory>
#include "rust/cxx.h"
#include "vrv/toolkit.h"

namespace vrv_rs {

std::unique_ptr<vrv::Toolkit> new_toolkit(bool init_fonts);

rust::String get_version(const vrv::Toolkit &tk);

bool set_resource_path(vrv::Toolkit &tk, rust::Str path);
rust::String get_resource_path(const vrv::Toolkit &tk);

rust::String get_id(vrv::Toolkit &tk);

bool load_data(vrv::Toolkit &tk, rust::Str data);
bool load_file(vrv::Toolkit &tk, rust::Str filename);
bool load_zip_data_buffer(vrv::Toolkit &tk, rust::Slice<const uint8_t> data);

int page_count(vrv::Toolkit &tk);

rust::String get_options(const vrv::Toolkit &tk);
rust::String get_default_options(const vrv::Toolkit &tk);
rust::String get_available_options(const vrv::Toolkit &tk);
bool set_options(vrv::Toolkit &tk, rust::Str options);
void reset_options(vrv::Toolkit &tk);
bool select(vrv::Toolkit &tk, rust::Str selection);

bool set_scale(vrv::Toolkit &tk, int32_t scale);
int32_t get_scale(vrv::Toolkit &tk);

bool set_input_from(vrv::Toolkit &tk, rust::Str format);
bool set_output_to(vrv::Toolkit &tk, rust::Str format);

void reset_xml_id_seed(vrv::Toolkit &tk, int32_t seed);

// Rendering surface — every method here returns an owned std::string upstream
// and Verovio offers no streaming overload, so the inevitable C++ allocation
// happens regardless. The Rust-side `_into` variants in the safe wrapper
// still eliminate the per-call `String` heap churn for the caller.
rust::String render_to_svg(vrv::Toolkit &tk, int32_t page_no, bool xml_declaration);
rust::String render_to_midi(vrv::Toolkit &tk); // base64-encoded MIDI
rust::String render_to_timemap(vrv::Toolkit &tk, rust::Str json_options);
rust::String render_to_expansion_map(vrv::Toolkit &tk);
rust::String render_to_pae(vrv::Toolkit &tk);

rust::String validate_pae(vrv::Toolkit &tk, rust::Str data);

rust::String get_mei(vrv::Toolkit &tk, rust::Str json_options);

rust::String get_descriptive_features(vrv::Toolkit &tk, rust::Str json_options);

void redo_layout(vrv::Toolkit &tk, rust::Str json_options);
void redo_page_pitch_pos_layout(vrv::Toolkit &tk);

rust::String get_elements_at_time(vrv::Toolkit &tk, int32_t millisec);

// Element introspection (inverse of get_elements_at_time / hit-testing).
int32_t get_page_with_element(vrv::Toolkit &tk, rust::Str xml_id);
int32_t get_time_for_element(vrv::Toolkit &tk, rust::Str xml_id);
rust::String get_midi_values_for_element(vrv::Toolkit &tk, rust::Str xml_id);
rust::String get_times_for_element(vrv::Toolkit &tk, rust::Str xml_id);
rust::String get_element_attr(vrv::Toolkit &tk, rust::Str xml_id);
rust::String get_notated_id_for_element(vrv::Toolkit &tk, rust::Str xml_id);
rust::String get_expansion_ids_for_element(vrv::Toolkit &tk, rust::Str xml_id);

// Process-global log control. Verovio's log threshold lives in a
// namespace-level variable (`vrv::logLevel`); the safe-wrapper crate
// gates this behind a Mutex so concurrent toolkits don't race on it.
void enable_log(int32_t level);

} // namespace vrv_rs
