/// The entrypoint for the **server** environment.
///
/// The [main] method will only be executed on the server during pre-rendering.
/// To run code on the client, check the `main.client.dart` file.
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
      title: '$siteTitle — $siteTagline',
      lang: 'en',
      base: '/',
      meta: const {
        'description': siteDescription,
      },
      head: [
        meta(attributes: {'property': 'og:type'}, content: 'website'),
        meta(attributes: {'property': 'og:site_name'}, content: siteTitle),
        meta(attributes: {'property': 'og:title'}, content: '$siteTitle — $siteTagline'),
        meta(attributes: {'property': 'og:description'}, content: siteDescription),
        meta(attributes: {'property': 'og:url'}, content: '$siteUrl/'),
        meta(attributes: {'property': 'og:image'}, content: ogImageUrl),
        meta(attributes: {'property': 'og:image:alt'}, content: ogImageAlt),
        meta(name: 'twitter:card', content: 'summary'),
        meta(name: 'twitter:title', content: '$siteTitle — $siteTagline'),
        meta(name: 'twitter:description', content: siteDescription),
        meta(name: 'twitter:image', content: ogImageUrl),
        meta(name: 'twitter:image:alt', content: ogImageAlt),
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
        link(href: '$siteUrl/', rel: 'canonical'),
        link(href: 'styles.css', rel: 'stylesheet'),
        link(href: 'favicon.svg', rel: 'icon', type: 'image/svg+xml'),
        link(href: 'favicon.ico', rel: 'icon'),
        link(href: 'apple-touch-icon.png', rel: 'apple-touch-icon'),
      ],
      body: const App(),
    ),
  );
}
