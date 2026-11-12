/// Where the desktop tab strip or list sits around the web view.
///
/// Mirrors `morph_core::settings::TabPosition`. Default is [top].
enum MorphTabPosition {
  top,
  bottom,
  left,
  right;

  /// Wire format stored in `settings.json`.
  String get key => name;

  /// Parse a stored or UI value. Unknown values fall back to [top].
  static MorphTabPosition fromKey(String? value) {
    switch (value?.trim().toLowerCase()) {
      case 'bottom':
        return MorphTabPosition.bottom;
      case 'left':
        return MorphTabPosition.left;
      case 'right':
        return MorphTabPosition.right;
      default:
        return MorphTabPosition.top;
    }
  }

  bool get isVertical =>
      this == MorphTabPosition.left || this == MorphTabPosition.right;
}
