import 'dart:convert';
import 'dart:typed_data';

/// What a surface check found for one web view.
enum MorphSurfaceVerdict {
  /// The page has content and the captured frame shows it.
  healthy,

  /// The page has content but the frame is one flat colour: the surface is
  /// not painting (white, frozen web view).
  blank,

  /// The renderer process died; the view must be rebuilt.
  rendererGone,

  /// Nothing to judge yet (no page, still loading, no capture available).
  unknown,
}

/// The page-side half of a surface check, read with [morphSurfaceProbeScript].
class MorphSurfaceProbe {
  const MorphSurfaceProbe({
    required this.readyState,
    required this.textLength,
    required this.mediaCount,
    required this.width,
    required this.height,
  });

  final String readyState;
  final int textLength;
  final int mediaCount;
  final int width;
  final int height;

  /// True when a correctly painting surface would show something other than
  /// a single flat colour.
  bool get hasVisibleContent =>
      readyState != 'loading' &&
      width > 0 &&
      height > 0 &&
      (textLength > 0 || mediaCount > 0);

  /// Parses the probe result. Android returns the JSON string itself, other
  /// backends may return it JSON-quoted. Returns null for anything else.
  static MorphSurfaceProbe? tryParse(Object? raw) {
    if (raw == null) {
      return null;
    }
    Object? decoded = raw;
    try {
      for (var i = 0; i < 2 && decoded is String; i++) {
        decoded = jsonDecode(decoded);
      }
    } catch (_) {
      return null;
    }
    if (decoded is! Map) {
      return null;
    }
    int asInt(Object? v) => v is num ? v.toInt() : 0;
    return MorphSurfaceProbe(
      readyState: decoded['readyState']?.toString() ?? '',
      textLength: asInt(decoded['text']),
      mediaCount: asInt(decoded['media']),
      width: asInt(decoded['w']),
      height: asInt(decoded['h']),
    );
  }
}

/// Reads whether the visible part of the page has anything a painting
/// surface would show: text laid out inside the viewport, or visible media.
const morphSurfaceProbeScript = '''(function () {
  var d = document, e = d.documentElement, b = d.body;
  var vw = window.innerWidth, vh = window.innerHeight;
  function onScreen(r) {
    return r.width > 0 && r.height > 0 && r.bottom > 0 && r.right > 0 &&
      r.top < vh && r.left < vw;
  }
  function shown(el) {
    var s = el && window.getComputedStyle(el);
    return !!s && s.visibility !== 'hidden' && s.display !== 'none' &&
      parseFloat(s.opacity || '1') > 0.05;
  }
  var text = 0, media = 0;
  if (b) {
    var w = d.createTreeWalker(b, NodeFilter.SHOW_TEXT, null);
    var range = d.createRange(), n, seen = 0;
    while ((n = w.nextNode()) && seen < 3000 && text < 40) {
      seen++;
      var t = (n.nodeValue || '').trim();
      if (!t || !shown(n.parentElement)) { continue; }
      range.selectNodeContents(n);
      if (onScreen(range.getBoundingClientRect())) { text += t.length; }
    }
    var m = d.querySelectorAll('img,svg,canvas,video,iframe');
    for (var i = 0; i < m.length && i < 500 && media < 3; i++) {
      if (shown(m[i]) && onScreen(m[i].getBoundingClientRect())) { media++; }
    }
  }
  return JSON.stringify({
    readyState: d.readyState,
    text: text,
    media: media,
    w: e ? e.clientWidth : 0,
    h: e ? e.clientHeight : 0
  });
})()''';

/// Forces the web view to produce a fresh compositor frame without changing
/// the page or firing scroll events: the root is promoted to its own layer
/// for two animation frames, then restored. A new frame is what a user's
/// touch provides when it "wakes" a stale surface.
const morphSurfaceRepaintScript = '''(function () {
  var e = document.documentElement;
  if (!e) { return 'none'; }
  var prev = e.style.getPropertyValue('will-change');
  e.style.setProperty('will-change', 'transform');
  requestAnimationFrame(function () {
    requestAnimationFrame(function () {
      if (prev) { e.style.setProperty('will-change', prev); }
      else { e.style.removeProperty('will-change'); }
    });
  });
  return 'ok';
})()''';

/// True when [rgba] (tightly packed RGBA8888, decoded from the capture) is
/// a single flat colour. [tolerance] absorbs JPEG rounding of a flat frame.
///
/// A surface that is not painting is exactly uniform, so the defaults are
/// strict: a few pixels of small text on a white page (a bare
/// "403 Forbidden") must count as content, never as a blank surface.
bool morphFrameIsFlat(
  Uint8List rgba, {
  int tolerance = 2,
  double maxOddFraction = 0,
}) {
  final pixels = rgba.length ~/ 4;
  if (pixels == 0) {
    return true;
  }
  final r = rgba[0], g = rgba[1], b = rgba[2];
  var odd = 0;
  final allowed = (pixels * maxOddFraction).floor();
  for (var i = 0; i < pixels; i++) {
    final o = i * 4;
    if ((rgba[o] - r).abs() > tolerance ||
        (rgba[o + 1] - g).abs() > tolerance ||
        (rgba[o + 2] - b).abs() > tolerance) {
      odd++;
      if (odd > allowed) {
        return false;
      }
    }
  }
  return true;
}

/// Combines the page probe and the captured frame into a verdict.
MorphSurfaceVerdict morphClassifySurface({
  required MorphSurfaceProbe? probe,
  required bool? frameIsFlat,
}) {
  if (probe == null || frameIsFlat == null || !probe.hasVisibleContent) {
    return MorphSurfaceVerdict.unknown;
  }
  return frameIsFlat ? MorphSurfaceVerdict.blank : MorphSurfaceVerdict.healthy;
}
