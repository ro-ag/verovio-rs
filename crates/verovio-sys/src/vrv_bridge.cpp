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

rust::String get_resource_path(const vrv::Toolkit &tk) {
    return rust::String(tk.GetResourcePath());
}

rust::String get_id(vrv::Toolkit &tk) {
    return rust::String(tk.GetID());
}

bool load_data(vrv::Toolkit &tk, rust::Str data) {
    return tk.LoadData(std::string(data));
}

bool load_file(vrv::Toolkit &tk, rust::Str filename) {
    return tk.LoadFile(std::string(filename));
}

bool load_zip_data_buffer(vrv::Toolkit &tk, rust::Slice<const uint8_t> data) {
    // Upstream is `bool LoadZipDataBuffer(const unsigned char *, int)`.
    // cxx slices are non-owning views; safe to forward the pointer + length.
    return tk.LoadZipDataBuffer(data.data(), static_cast<int>(data.size()));
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

rust::String get_available_options(const vrv::Toolkit &tk) {
    return rust::String(tk.GetAvailableOptions());
}

bool set_options(vrv::Toolkit &tk, rust::Str options) {
    return tk.SetOptions(std::string(options));
}

void reset_options(vrv::Toolkit &tk) {
    tk.ResetOptions();
}

bool select(vrv::Toolkit &tk, rust::Str selection) {
    return tk.Select(std::string(selection));
}

bool set_scale(vrv::Toolkit &tk, int32_t scale) {
    return tk.SetScale(scale);
}

int32_t get_scale(vrv::Toolkit &tk) {
    return tk.GetScale();
}

bool set_input_from(vrv::Toolkit &tk, rust::Str format) {
    return tk.SetInputFrom(std::string(format));
}

bool set_output_to(vrv::Toolkit &tk, rust::Str format) {
    return tk.SetOutputTo(std::string(format));
}

void reset_xml_id_seed(vrv::Toolkit &tk, int32_t seed) {
    tk.ResetXmlIdSeed(seed);
}

rust::String render_to_svg(vrv::Toolkit &tk, int32_t page_no, bool xml_declaration) {
    return rust::String(tk.RenderToSVG(page_no, xml_declaration));
}

rust::String render_to_midi(vrv::Toolkit &tk) {
    return rust::String(tk.RenderToMIDI());
}

rust::String render_to_expansion_map(vrv::Toolkit &tk) {
    return rust::String(tk.RenderToExpansionMap());
}

rust::String render_to_pae(vrv::Toolkit &tk) {
    return rust::String(tk.RenderToPAE());
}

rust::String validate_pae(vrv::Toolkit &tk, rust::Str data) {
    return rust::String(tk.ValidatePAE(std::string(data)));
}

rust::String render_to_timemap(vrv::Toolkit &tk, rust::Str json_options) {
    return rust::String(tk.RenderToTimemap(std::string(json_options)));
}

rust::String get_mei(vrv::Toolkit &tk, rust::Str json_options) {
    return rust::String(tk.GetMEI(std::string(json_options)));
}

rust::String get_descriptive_features(vrv::Toolkit &tk, rust::Str json_options) {
    return rust::String(tk.GetDescriptiveFeatures(std::string(json_options)));
}

void redo_layout(vrv::Toolkit &tk, rust::Str json_options) {
    tk.RedoLayout(std::string(json_options));
}

void redo_page_pitch_pos_layout(vrv::Toolkit &tk) {
    tk.RedoPagePitchPosLayout();
}

rust::String get_elements_at_time(vrv::Toolkit &tk, int32_t millisec) {
    return rust::String(tk.GetElementsAtTime(millisec));
}

int32_t get_page_with_element(vrv::Toolkit &tk, rust::Str xml_id) {
    return tk.GetPageWithElement(std::string(xml_id));
}

int32_t get_time_for_element(vrv::Toolkit &tk, rust::Str xml_id) {
    return tk.GetTimeForElement(std::string(xml_id));
}

rust::String get_midi_values_for_element(vrv::Toolkit &tk, rust::Str xml_id) {
    return rust::String(tk.GetMIDIValuesForElement(std::string(xml_id)));
}

rust::String get_times_for_element(vrv::Toolkit &tk, rust::Str xml_id) {
    return rust::String(tk.GetTimesForElement(std::string(xml_id)));
}

rust::String get_element_attr(vrv::Toolkit &tk, rust::Str xml_id) {
    return rust::String(tk.GetElementAttr(std::string(xml_id)));
}

rust::String get_notated_id_for_element(vrv::Toolkit &tk, rust::Str xml_id) {
    return rust::String(tk.GetNotatedIdForElement(std::string(xml_id)));
}

rust::String get_expansion_ids_for_element(vrv::Toolkit &tk, rust::Str xml_id) {
    return rust::String(tk.GetExpansionIdsForElement(std::string(xml_id)));
}

void enable_log(int32_t level) {
    vrv::EnableLog(static_cast<vrv::LogLevel>(level));
}

} // namespace vrv_rs
