#include "my_application.h"
#include "display_backend.h"

int main(int argc, char** argv) {
  // Before GtkApplication / gtk_init: on NVIDIA Wayland, prefer the X11
  // backend so Impeller can create a GL context. See display_backend.h.
  morph_prepare_gdk_backend();
  g_autoptr(MyApplication) app = my_application_new();
  return g_application_run(G_APPLICATION(app), argc, argv);
}
