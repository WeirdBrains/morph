import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../theme/morph_theme.dart';

/// Match count and the 1-based current match (0 when nothing is selected).
class MorphFindResult {
  const MorphFindResult({this.total = 0, this.current = 0});

  static const none = MorphFindResult();

  final int total;
  final int current;
}

/// Page-side find, the same on every backend: counts matches in the page
/// text and moves the selection with `window.find`, which also scrolls it into
/// view. Returns `"total/current"`.
String morphFindScript(
  String query, {
  bool backward = false,
  bool reset = false,
}) {
  return '(function(q,back,reset){'
      'var s=window.__morphFind||(window.__morphFind={q:"",i:0});'
      'var sel=window.getSelection();'
      'if(reset||s.q!==q){s.q=q;s.i=0;if(sel){sel.removeAllRanges();}}'
      'if(!q){return "0/0";}'
      'var t=((document.body&&document.body.innerText)||"").toLowerCase();'
      'var n=q.toLowerCase(),total=0,p=0;'
      'while((p=t.indexOf(n,p))!==-1){total++;p+=n.length;}'
      'if(!total){s.i=0;return "0/0";}'
      'if(!window.find(q,false,back,true,false,false,false)){return total+"/0";}'
      's.i=back?(s.i<=1?total:s.i-1):(s.i>=total?1:s.i+1);'
      'return total+"/"+s.i;'
      '})(${jsonEncode(query)},$backward,$reset)';
}

/// Clears the find selection and state left in the page.
const morphFindClearScript =
    '(function(){var s=window.getSelection();if(s){s.removeAllRanges();}'
    'delete window.__morphFind;return "";})()';

/// Reads a [morphFindScript] reply. Backends may wrap the string in quotes.
MorphFindResult parseMorphFindResult(String? reply) {
  final match = RegExp(r'(\d+)/(\d+)').firstMatch(reply ?? '');
  if (match == null) {
    return MorphFindResult.none;
  }
  final total = int.parse(match.group(1)!);
  final current = int.parse(match.group(2)!);
  return MorphFindResult(
    total: total,
    current: current > total ? total : current,
  );
}

/// Find-in-page bar shown above the page.
class MorphFindBar extends StatefulWidget {
  const MorphFindBar({super.key, required this.onFind, required this.onClose});

  /// Runs a search. [reset] is true when the query text changed.
  final Future<MorphFindResult> Function(
    String query, {
    required bool backward,
    required bool reset,
  })
  onFind;
  final VoidCallback onClose;

  @override
  State<MorphFindBar> createState() => _MorphFindBarState();
}

class _MorphFindBarState extends State<MorphFindBar> {
  final _query = TextEditingController();
  final _focus = FocusNode();
  MorphFindResult _result = MorphFindResult.none;
  int _generation = 0;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) {
        _focus.requestFocus();
      }
    });
  }

  @override
  void dispose() {
    _query.dispose();
    _focus.dispose();
    super.dispose();
  }

  Future<void> _run({bool backward = false, bool reset = false}) async {
    final generation = ++_generation;
    final text = _query.text;
    MorphFindResult result;
    try {
      result = await widget.onFind(text, backward: backward, reset: reset);
    } catch (_) {
      result = MorphFindResult.none;
    }
    // Drop replies that a newer keystroke has already replaced.
    if (mounted && generation == _generation) {
      setState(() => _result = result);
    }
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final has = _query.text.isNotEmpty;
    final label = !has
        ? ''
        : _result.total == 0
        ? 'No matches'
        : '${_result.current}/${_result.total}';
    return Material(
      key: const Key('find-bar'),
      color: colors.surface,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(12, 4, 4, 4),
        child: CallbackShortcuts(
          bindings: {
            const SingleActivator(LogicalKeyboardKey.escape): widget.onClose,
            const SingleActivator(LogicalKeyboardKey.enter, shift: true): () =>
                _run(backward: true),
          },
          child: Row(
            children: [
              const Icon(Icons.search, size: 20, color: MorphColors.cyan),
              const SizedBox(width: 8),
              Expanded(
                child: TextField(
                  key: const Key('find-field'),
                  controller: _query,
                  focusNode: _focus,
                  textInputAction: TextInputAction.search,
                  decoration: const InputDecoration(
                    hintText: 'Find in page',
                    isDense: true,
                    border: InputBorder.none,
                    enabledBorder: InputBorder.none,
                    focusedBorder: InputBorder.none,
                    filled: false,
                  ),
                  onChanged: (_) => _run(reset: true),
                  onSubmitted: (_) {
                    _run();
                    _focus.requestFocus();
                  },
                ),
              ),
              Text(
                label,
                key: const Key('find-count'),
                style: MorphTheme.monoStyle(
                  color: _result.total == 0 && has
                      ? MorphColors.magenta
                      : colors.onSurface.withValues(alpha: 0.7),
                  size: 12,
                ),
              ),
              IconButton(
                key: const Key('find-prev'),
                tooltip: 'Previous match',
                icon: const Icon(Icons.keyboard_arrow_up),
                onPressed: has ? () => _run(backward: true) : null,
              ),
              IconButton(
                key: const Key('find-next'),
                tooltip: 'Next match',
                icon: const Icon(Icons.keyboard_arrow_down),
                onPressed: has ? () => _run() : null,
              ),
              IconButton(
                key: const Key('find-close'),
                tooltip: 'Close find',
                icon: const Icon(Icons.close),
                onPressed: widget.onClose,
              ),
            ],
          ),
        ),
      ),
    );
  }
}
