import 'dart:async';

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import '../api.dart';
import '../theme/morph_theme.dart';
import 'password_field.dart';

/// Settings > Passwords: the saver switch, "ask again", and saved logins.
class MorphPasswordsPage extends StatefulWidget {
  const MorphPasswordsPage({
    super.key,
    required this.api,
    required this.profileId,
    this.home,
  });

  final MorphApi api;
  final String profileId;
  final String? home;

  @override
  State<MorphPasswordsPage> createState() => _MorphPasswordsPageState();
}

class _MorphPasswordsPageState extends State<MorphPasswordsPage> {
  MorphSaverOffer? _offer;
  List<MorphSavedLogin> _logins = const [];
  String? _error;
  String? _note;
  String? _shownId;
  String? _shownPassword;
  Timer? _hide;
  String? _confirmDelete;
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    unawaited(_reload());
  }

  @override
  void dispose() {
    _hide?.cancel();
    super.dispose();
  }

  Future<void> _reload() async {
    try {
      final offer = await widget.api.passwordSaverOffer(
        widget.profileId,
        home: widget.home,
      );
      var logins = const <MorphSavedLogin>[];
      if (offer.available) {
        logins = await widget.api.listSavedLogins(
          widget.profileId,
          home: widget.home,
        );
      }
      if (!mounted) {
        return;
      }
      setState(() {
        _offer = offer;
        _logins = logins;
        _error = null;
        if (_shownId != null && logins.every((l) => l.id != _shownId)) {
          _shownId = null;
          _shownPassword = null;
        }
      });
    } catch (e) {
      if (mounted) {
        setState(() => _error = _friendly(e));
      }
    }
  }

  String _friendly(Object e) {
    final text = e.toString();
    // Drop a "StateError: " / "Exception: " prefix so the UI stays short.
    return text.replaceFirst(RegExp(r'^[^:]+:\s*'), '');
  }

  Future<void> _toggleEnabled() async {
    final offer = _offer;
    if (offer == null || _busy) {
      return;
    }
    setState(() => _busy = true);
    try {
      final next = await widget.api.setPasswordSaverEnabled(
        widget.profileId,
        !offer.enabled,
        home: widget.home,
      );
      if (!mounted) {
        return;
      }
      setState(() {
        _offer = next;
        _note = next.enabled ? 'Morph offers to save and fill passwords.' : 'Morph no longer offers to save or fill passwords. Saved ones stay.';
        _error = null;
        _busy = false;
      });
    } catch (e) {
      if (mounted) {
        setState(() {
          _error = _friendly(e);
          _busy = false;
        });
      }
    }
  }

  Future<void> _toggleAskAgain() async {
    final offer = _offer;
    if (offer == null || _busy) {
      return;
    }
    if (offer.askAgain) {
      final password = await _askPassword(
        'Enter the profile password to stop asking before every fill.',
      );
      if (password == null) {
        return;
      }
      await _setAskAgain(false, password);
      return;
    }
    await _setAskAgain(true, null);
  }

  Future<void> _setAskAgain(bool on, String? password) async {
    setState(() => _busy = true);
    try {
      final next = await widget.api.setPasswordSaverAskAgain(
        widget.profileId,
        on,
        password: password,
        home: widget.home,
      );
      if (!mounted) {
        return;
      }
      setState(() {
        _offer = next;
        _note = next.askAgain
            ? 'Morph asks for the profile password before every fill.'
            : 'Morph no longer asks for the profile password before filling.';
        _error = null;
        _busy = false;
      });
    } catch (e) {
      if (mounted) {
        setState(() {
          _error = _friendly(e);
          _busy = false;
        });
      }
    }
  }

  Future<String?> _askPassword(String reason) {
    return showDialog<String>(
      context: context,
      builder: (context) => _PasswordPrompt(reason: reason),
    );
  }

  Future<void> _reveal(MorphSavedLogin login) async {
    if (_shownId == login.id) {
      _hide?.cancel();
      setState(() {
        _shownId = null;
        _shownPassword = null;
      });
      return;
    }
    final password = await _askPassword(
      'Enter the profile password to show this saved password.',
    );
    if (password == null) {
      return;
    }
    try {
      final secret = await widget.api.revealSavedPassword(
        widget.profileId,
        login.id,
        password: password,
        home: widget.home,
      );
      if (!mounted) {
        return;
      }
      _hide?.cancel();
      setState(() {
        _shownId = login.id;
        _shownPassword = secret;
        _error = null;
      });
      _hide = Timer(const Duration(seconds: 60), () {
        if (mounted) {
          setState(() {
            _shownId = null;
            _shownPassword = null;
          });
        }
      });
    } catch (e) {
      if (mounted) {
        setState(() => _error = _friendly(e));
      }
    }
  }

  Future<void> _copy(MorphSavedLogin login) async {
    var secret = _shownId == login.id ? _shownPassword : null;
    if (secret == null) {
      final password = await _askPassword(
        'Enter the profile password to copy this saved password.',
      );
      if (password == null) {
        return;
      }
      try {
        secret = await widget.api.revealSavedPassword(
          widget.profileId,
          login.id,
          password: password,
          home: widget.home,
        );
      } catch (e) {
        if (mounted) {
          setState(() => _error = _friendly(e));
        }
        return;
      }
    }
    await Clipboard.setData(ClipboardData(text: secret));
    if (mounted) {
      setState(() => _note = 'Password copied.');
    }
  }

  Future<void> _delete(MorphSavedLogin login) async {
    if (_confirmDelete != login.id) {
      setState(() => _confirmDelete = login.id);
      return;
    }
    try {
      await widget.api.deleteSavedLogin(
        widget.profileId,
        login.id,
        home: widget.home,
      );
      if (!mounted) {
        return;
      }
      setState(() {
        _confirmDelete = null;
        _note = 'Deleted the saved login.';
      });
      await _reload();
    } catch (e) {
      if (mounted) {
        setState(() => _error = _friendly(e));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final offer = _offer;
    return Scaffold(
      key: const Key('passwords-page'),
      appBar: AppBar(title: const Text('Passwords')),
      body: offer == null && _error == null
          ? const Center(child: CircularProgressIndicator())
          : ListView(
              children: [
                if (_error != null)
                  ListTile(
                    key: const Key('passwords-error'),
                    leading: const Icon(
                      Icons.error_outline,
                      color: MorphColors.magenta,
                    ),
                    title: Text(_error!),
                  ),
                if (_note != null)
                  ListTile(
                    key: const Key('passwords-note'),
                    title: Text(_note!),
                  ),
                if (offer != null && !offer.available)
                  ListTile(
                    key: const Key('passwords-unavailable'),
                    leading: const Icon(Icons.info_outline),
                    title: Text(offer.unavailable),
                  )
                else if (offer != null) ...[
                  SwitchListTile(
                    key: const Key('passwords-enabled'),
                    title: const Text('Offer to save passwords'),
                    subtitle: const Text(
                      'When you sign in on a site, Morph can save the login in this profile and offer to fill it next time. Only https sites, and only on the exact site it was saved for.',
                    ),
                    value: offer.enabled,
                    onChanged: _busy
                        ? null
                        : (_) => unawaited(_toggleEnabled()),
                  ),
                  SwitchListTile(
                    key: const Key('passwords-ask-again'),
                    title: const Text('Ask again before filling'),
                    subtitle: const Text(
                      'Ask for this profile\'s password before filling a saved login. Turning this off needs the profile password.',
                    ),
                    value: offer.askAgain,
                    onChanged: _busy
                        ? null
                        : (_) => unawaited(_toggleAskAgain()),
                  ),
                  const Padding(
                    padding: EdgeInsets.fromLTRB(16, 20, 16, 6),
                    child: Text(
                      'SAVED LOGINS',
                      style: TextStyle(
                        color: MorphColors.cyan,
                        fontSize: 12,
                        fontWeight: FontWeight.w700,
                        letterSpacing: 1.2,
                      ),
                    ),
                  ),
                  if (_logins.isEmpty)
                    const ListTile(
                      key: Key('passwords-empty'),
                      title: Text('No saved logins in this profile yet.'),
                    )
                  else
                    for (final login in _logins) _loginTile(login),
                ],
              ],
            ),
    );
  }

  Widget _loginTile(MorphSavedLogin login) {
    final shown = _shownId == login.id;
    final confirm = _confirmDelete == login.id;
    final who = login.username.isEmpty
        ? login.origin
        : '${login.username} · ${login.origin}';
    return Padding(
      key: Key('passwords-login-${login.id}'),
      padding: const EdgeInsets.fromLTRB(16, 8, 16, 8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(who),
          if (shown)
            Padding(
              padding: const EdgeInsets.only(top: 4),
              child: Text(
                _shownPassword ?? '',
                key: Key('passwords-secret-${login.id}'),
              ),
            ),
          OverflowBar(
            spacing: 4,
            children: [
              TextButton(
                key: Key('passwords-show-${login.id}'),
                onPressed: () => unawaited(_reveal(login)),
                child: Text(shown ? 'Hide' : 'Show'),
              ),
              TextButton(
                key: Key('passwords-copy-${login.id}'),
                onPressed: () => unawaited(_copy(login)),
                child: const Text('Copy'),
              ),
              TextButton(
                key: Key('passwords-delete-${login.id}'),
                onPressed: () => unawaited(_delete(login)),
                child: Text(confirm ? 'Delete?' : 'Delete'),
              ),
            ],
          ),
        ],
      ),
    );
  }
}

class _PasswordPrompt extends StatefulWidget {
  const _PasswordPrompt({required this.reason});

  final String reason;

  @override
  State<_PasswordPrompt> createState() => _PasswordPromptState();
}

class _PasswordPromptState extends State<_PasswordPrompt> {
  final _controller = TextEditingController();

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('Profile password'),
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(widget.reason),
            const SizedBox(height: 12),
            MorphPasswordField(
              key: const Key('passwords-profile-password'),
              controller: _controller,
              label: 'Profile password',
              textInputAction: TextInputAction.done,
              onSubmitted: (value) => Navigator.of(context).pop(value),
            ),
          ],
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: const Text('Cancel'),
        ),
        FilledButton(
          key: const Key('passwords-password-ok'),
          onPressed: () => Navigator.of(context).pop(_controller.text),
          child: const Text('Continue'),
        ),
      ],
    );
  }
}
