#ifndef MORPH_DISPLAY_BACKEND_H_
#define MORPH_DISPLAY_BACKEND_H_

// Choose a GDK backend before gtk_init. Returns a string to set as
// GDK_BACKEND, or NULL to leave the environment unchanged.
//
// If gdk_backend is already set (non-empty), returns NULL so an explicit
// choice is respected. If this is a Wayland session (wayland_display
// non-empty) with XWayland available (x11_display non-empty) and the NVIDIA
// proprietary driver is present, returns "x11". Flutter's Linux embedder
// (Impeller OpenGLES) needs a GDK GL context; on NVIDIA the GTK 3 Wayland
// backend reports "The current backend does not support OpenGL" and the
// process aborts. The X11 backend on XWayland can create one. Wayland stays
// the default on other GPUs, and when no X11 display is available.
const char* morph_preferred_gdk_backend(const char* gdk_backend,
                                        const char* wayland_display,
                                        const char* x11_display,
                                        int has_nvidia);

// True when the NVIDIA proprietary kernel driver is present. Same probe as
// morph_has_nvidia_proprietary_driver in the web view plugin.
int morph_runner_has_nvidia_proprietary_driver(void);

// Apply the preference to the process environment. Must run before
// GtkApplication / gtk_init. Safe to call more than once.
void morph_prepare_gdk_backend(void);

#endif  // MORPH_DISPLAY_BACKEND_H_
