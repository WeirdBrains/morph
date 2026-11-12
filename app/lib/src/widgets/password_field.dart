import 'package:flutter/material.dart';

/// The one password input Morph uses: obscured by default, with an eye
/// button to show or hide what was typed. Every password and confirmation
/// field in the app goes through this (a test keeps it that way).
class MorphPasswordField extends StatefulWidget {
  const MorphPasswordField({
    super.key,
    required this.controller,
    required this.label,
    this.autofillHints,
    this.onSubmitted,
    this.textInputAction,
  });

  final TextEditingController controller;
  final String label;
  final Iterable<String>? autofillHints;
  final ValueChanged<String>? onSubmitted;
  final TextInputAction? textInputAction;

  @override
  State<MorphPasswordField> createState() => _MorphPasswordFieldState();
}

class _MorphPasswordFieldState extends State<MorphPasswordField> {
  bool _visible = false;

  @override
  Widget build(BuildContext context) {
    return TextField(
      controller: widget.controller,
      obscureText: !_visible,
      autocorrect: false,
      enableSuggestions: false,
      enableIMEPersonalizedLearning: false,
      keyboardType: TextInputType.visiblePassword,
      autofillHints: widget.autofillHints,
      textInputAction: widget.textInputAction,
      onSubmitted: widget.onSubmitted,
      decoration: InputDecoration(
        labelText: widget.label,
        suffixIcon: IconButton(
          key: const Key('password-visibility'),
          tooltip: _visible ? 'Hide password' : 'Show password',
          icon: Icon(_visible ? Icons.visibility_off : Icons.visibility),
          onPressed: () => setState(() => _visible = !_visible),
        ),
      ),
    );
  }
}
