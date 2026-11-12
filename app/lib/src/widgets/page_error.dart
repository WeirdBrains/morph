import 'dart:async';

import 'package:flutter/material.dart';
import 'package:morph_webview/morph_webview.dart';

import '../incoming_links.dart';
import '../theme/morph_theme.dart';
import 'brand.dart';

/// Title and explanation for a page that failed to load.
(String, String) morphLoadErrorText(MorphLoadError error) {
  final host = Uri.tryParse(error.url)?.host ?? '';
  final site = host.isEmpty ? 'This site' : host;
  return switch (error.kind) {
    MorphLoadErrorKind.certificate => (
      "This connection isn't private",
      '$site sent a security certificate Morph cannot trust, so the page '
          'was not loaded. ${error.message} Check the address for a typo.',
    ),
    MorphLoadErrorKind.notFound => (
      "Can't find $site",
      'The address may be misspelled, or the site no longer exists.',
    ),
    MorphLoadErrorKind.unreachable => (
      "Can't reach $site",
      'The site refused the connection or the network is down. '
          'Check your connection and try again.',
    ),
    MorphLoadErrorKind.timedOut => (
      '$site took too long to respond',
      'The site may be busy or your connection slow. Try again.',
    ),
    MorphLoadErrorKind.other => (
      "This page couldn't load",
      error.message.isEmpty ? 'Try again in a moment.' : error.message,
    ),
  };
}

/// Covers one tab with Morph's error page while its last main-frame load
/// failed, instead of the engine's plain text over a blank frame.
class MorphLoadErrorGate extends StatelessWidget {
  const MorphLoadErrorGate({
    super.key,
    required this.controller,
    required this.child,
  });

  final MorphWebViewController controller;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    return ValueListenableBuilder<MorphLoadError?>(
      valueListenable: controller.loadError,
      builder: (context, error, _) {
        return Stack(
          fit: StackFit.expand,
          children: [
            child,
            if (error != null)
              Listener(
                behavior: HitTestBehavior.opaque,
                child: MorphPageError(
                  error: error,
                  onRetry: () => unawaited(controller.reload()),
                  onSearch: error.kind == MorphLoadErrorKind.notFound
                      ? () => unawaited(
                          controller.loadUrl(
                            MorphIncomingLinks.searchUrl(
                              Uri.tryParse(error.url)?.host ?? error.url,
                            ),
                          ),
                        )
                      : null,
                ),
              ),
          ],
        );
      },
    );
  }
}

/// The per-tab load-error panel.
class MorphPageError extends StatelessWidget {
  const MorphPageError({
    super.key,
    required this.error,
    required this.onRetry,
    this.onSearch,
  });

  final MorphLoadError error;
  final VoidCallback onRetry;
  final VoidCallback? onSearch;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final colors = theme.colorScheme;
    final (title, body) = morphLoadErrorText(error);
    return Material(
      key: const Key('page-error'),
      color: theme.scaffoldBackgroundColor,
      child: SafeArea(
        child: Center(
          child: SingleChildScrollView(
            padding: const EdgeInsets.symmetric(horizontal: 32, vertical: 24),
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 460),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Icon(
                    error.kind == MorphLoadErrorKind.certificate
                        ? Icons.gpp_bad_outlined
                        : Icons.cloud_off_outlined,
                    size: 48,
                    color: colors.primary,
                  ),
                  const SizedBox(height: 20),
                  const MorphGlitchRule(),
                  const SizedBox(height: 20),
                  Text(
                    title,
                    textAlign: TextAlign.center,
                    style: theme.textTheme.titleLarge,
                  ),
                  const SizedBox(height: 8),
                  Text(
                    body,
                    key: const Key('page-error-body'),
                    textAlign: TextAlign.center,
                    style: MorphTheme.monoStyle(
                      color: colors.onSurfaceVariant,
                      size: 13,
                    ),
                  ),
                  const SizedBox(height: 24),
                  Wrap(
                    spacing: 12,
                    runSpacing: 8,
                    alignment: WrapAlignment.center,
                    children: [
                      if (onSearch != null)
                        OutlinedButton(
                          key: const Key('page-error-search'),
                          onPressed: onSearch,
                          child: const Text('Search instead'),
                        ),
                      FilledButton.icon(
                        key: const Key('page-error-retry'),
                        onPressed: onRetry,
                        icon: const Icon(Icons.refresh),
                        label: const Text('Try again'),
                      ),
                    ],
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}
