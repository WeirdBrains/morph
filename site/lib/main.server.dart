/// The entrypoint for the **server** environment.
///
/// The [main] method will only be executed on the server during pre-rendering.
/// Per-page title, description, canonical and Open Graph tags come from `Layout`.
library;

import 'package:jaspr/dom.dart';
import 'package:jaspr/server.dart';

import 'app.dart';
import 'main.server.options.dart';
import 'site.dart';

void main() {
  Jaspr.initializeApp(
    options: defaultServerOptions,
  );

  runApp(
    Document(
      title: siteTitle,
      lang: 'en',
      base: '/',
      head: [
        meta(
          name: 'theme-color',
          content: '#0D0D12',
          attributes: {'media': '(prefers-color-scheme: dark)'},
        ),
        meta(
          name: 'theme-color',
          content: '#F4F5F8',
          attributes: {'media': '(prefers-color-scheme: light)'},
        ),
        link(href: 'styles.css', rel: 'stylesheet'),
        link(href: 'favicon.svg', rel: 'icon', type: 'image/svg+xml'),
        link(href: 'favicon.ico', rel: 'icon'),
        link(href: 'apple-touch-icon.png', rel: 'apple-touch-icon'),
      ],
      body: const App(),
    ),
  );
}
