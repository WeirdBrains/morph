#!/bin/sh
# Morph Linux x64. Runs from this folder and installs nothing.
# Data: $MORPH_HOME if set, else ~/.morph (your normal Morph profiles).
#
# The Morph binary picks a GL-capable GDK backend before GTK starts. On
# NVIDIA Wayland it uses XWayland (GDK_BACKEND=x11), including when the
# session already set GDK_BACKEND=wayland,x11,*. MORPH_ALLOW_WAYLAND=1
# skips that; MORPH_GDK_BACKEND forces a backend. This launcher does not
# exec, so it can restore the terminal if Morph aborts or is interrupted.

here="$(cd "$(dirname "$0")" && pwd)"

restore_tty() {
  if [ -n "${MORPH_SAVED_STTY-}" ]; then
    stty "$MORPH_SAVED_STTY" 2>/dev/null || stty sane 2>/dev/null || true
  fi
}

MORPH_SAVED_STTY=
if [ -t 0 ]; then
  MORPH_SAVED_STTY=$(stty -g 2>/dev/null) || MORPH_SAVED_STTY=
  trap restore_tty EXIT INT HUP TERM
fi

# Stay in this shell so the EXIT trap runs after Morph exits or crashes.
"$here/morph" "$@"
exit $?
