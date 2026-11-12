#include "media_check.h"

#include <gmodule.h>

#include <cstring>

namespace {

struct Format {
  const char* name;
  const char* elements[6];
};

// Any one element per format is enough; hardware decoders count too.
const Format kFormats[] = {
    {"H.264", {"avdec_h264", "openh264dec", "nvh264dec", "vah264dec", "vulkanh264dec", nullptr}},
    {"VP9", {"vp9dec", "avdec_vp9", "nvvp9dec", "vavp9dec", "vulkanvp9dec", nullptr}},
    {"AV1", {"av1dec", "dav1ddec", "avdec_av1", "nvav1dec", "vaav1dec", nullptr}},
    {"Opus", {"opusdec", "avdec_opus", nullptr}},
    {"AAC", {"avdec_aac", "fdkaacdec", "faad", nullptr}},
    {"WebM", {"matroskademux", nullptr}},
    {"MP4", {"qtdemux", nullptr}},
};

typedef int (*GstInitCheck)(int*, char***, GError**);
typedef void* (*GstRegistryGet)(void);
typedef void* (*GstRegistryLookupFeature)(void*, const char*);
typedef void (*GstObjectUnref)(void*);

struct Gst {
  GstRegistryGet registry_get;
  GstRegistryLookupFeature lookup;
  GstObjectUnref unref;
};

gboolean has_element_in_registry(const gchar* name, gpointer user_data) {
  Gst* gst = static_cast<Gst*>(user_data);
  void* feature = gst->lookup(gst->registry_get(), name);
  if (feature == nullptr) {
    return FALSE;
  }
  gst->unref(feature);
  return TRUE;
}

}  // namespace

gchar* morph_media_missing_formats(MorphHasElementFunc has_element, gpointer user_data) {
  GString* missing = g_string_new(nullptr);
  for (const Format& format : kFormats) {
    gboolean found = FALSE;
    for (const char* const* element = format.elements; *element != nullptr; element++) {
      if (has_element(*element, user_data)) {
        found = TRUE;
        break;
      }
    }
    if (!found) {
      if (missing->len > 0) {
        g_string_append(missing, ", ");
      }
      g_string_append(missing, format.name);
    }
  }
  return g_string_free(missing, FALSE);
}

const gchar* morph_media_missing_formats_here(void) {
  static gsize once = 0;
  static gchar* result = nullptr;
  if (g_once_init_enter(&once)) {
    GModule* module = g_module_open("libgstreamer-1.0.so.0", G_MODULE_BIND_LAZY);
    gpointer init = nullptr;
    Gst gst = {};
    if (module != nullptr && g_module_symbol(module, "gst_init_check", &init) &&
        g_module_symbol(module, "gst_registry_get", reinterpret_cast<gpointer*>(&gst.registry_get)) &&
        g_module_symbol(module, "gst_registry_lookup_feature",
                        reinterpret_cast<gpointer*>(&gst.lookup)) &&
        g_module_symbol(module, "gst_object_unref", reinterpret_cast<gpointer*>(&gst.unref)) &&
        reinterpret_cast<GstInitCheck>(init)(nullptr, nullptr, nullptr)) {
      result = morph_media_missing_formats(has_element_in_registry, &gst);
    }
    // The module stays loaded: WebKit uses GStreamer in this process too.
    g_once_init_leave(&once, 1);
  }
  return result;
}

gchar* morph_media_crash_hint(const gchar* missing_formats, const gchar* disable_dmabuf_env) {
  GString* hint = g_string_new(nullptr);
  if (disable_dmabuf_env != nullptr && disable_dmabuf_env[0] != '\0' &&
      std::strcmp(disable_dmabuf_env, "0") != 0) {
    g_string_append(hint,
                    "WEBKIT_DISABLE_DMABUF_RENDERER is set; WebKitGTK crashes on video pages "
                    "with it. Unset it and restart Morph.");
  }
  if (missing_formats != nullptr && missing_formats[0] != '\0') {
    if (hint->len > 0) {
      g_string_append(hint, " ");
    }
    g_string_append_printf(hint,
                           "No GStreamer decoder for %s. Install gst-plugins-good, "
                           "gst-plugins-bad and gst-libav (Arch) or gstreamer1.0-plugins-good, "
                           "gstreamer1.0-plugins-bad and gstreamer1.0-libav (Debian, Ubuntu).",
                           missing_formats);
  }
  return g_string_free(hint, FALSE);
}
