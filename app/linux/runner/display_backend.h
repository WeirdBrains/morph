#ifndef MORPH_DISPLAY_BACKEND_H_
#define MORPH_DISPLAY_BACKEND_H_

// Choose a GDK backend before gtk_init. Returns a string to set as
// GDK_BACKEND, or NULL to leave the environment unchanged.
//
// Flutter's Linux embedder (Impeller OpenGLES) needs a GDK GL context. On
// NVIDIA, GTK 3's Wayland backend can fail to create one — sometimes after
// a first frame has already been presented — and Morph aborts with "The
// current backend does not support OpenGL".
//
// When the proprietary driver is present and an X11 display is available,
// this returns "x11" if Wayland would otherwise be used: GDK_BACKEND unset
// on a Wayland session, a list that asks for Wayland first (Omarchy /
// Hyprland set "wayland,x11,*"), or GDK_BACKEND=wayland. A backend whose
// first token is already "x11" is left alone. Wayland stays the default on
// other GPUs, and when no X11 display is available.
const char* morph_preferred_gdk_backend(const char* gdk_backend,
                                        const char* wayland_display,
                                        const char* x11_display,
                                        int has_nvidia);

// True when the NVIDIA proprietary kernel driver is present. Same probe as
// morph_has_nvidia_proprietary_driver in the web view plugin.
int morph_runner_has_nvidia_proprietary_driver(void);

// Apply the preference to the process environment, overwriting a session
// list such as "wayland,x11,*". Must run before GtkApplication / gtk_init.
// Safe to call more than once. MORPH_ALLOW_WAYLAND=1 skips the override.
// MORPH_GDK_BACKEND, if set, is copied to GDK_BACKEND as-is.
void morph_prepare_gdk_backend(void);

#endif  // MORPH_DISPLAY_BACKEND_H_
