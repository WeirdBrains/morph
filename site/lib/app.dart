import 'package:jaspr/dom.dart';
import 'package:jaspr/jaspr.dart';

import 'pages/home.dart';
import 'site.dart';

class App extends StatelessComponent {
  const App({super.key});

  @override
  Component build(BuildContext context) {
    return Component.fragment([
      a([Component.text('Skip to content')], href: '#content', classes: 'skip-link'),
      header([
        div([
          a(
            [
              Component.element(
                tag: 'picture',
                children: [
                  source(
                    attributes: {
                      'srcset': 'images/morph-wordmark-light.svg',
                      'media': '(prefers-color-scheme: light)',
                    },
                  ),
                  img(src: 'images/morph-wordmark.svg', alt: siteTitle, classes: 'wordmark'),
                ],
              ),
            ],
            href: '/',
            classes: 'logo',
          ),
          nav([
            a([Component.text('What Morph is')], href: '#about'),
            a([Component.text('Features')], href: '#features'),
            a([Component.text("How it's built")], href: '#built'),
            a([Component.text('Platforms')], href: '#platforms'),
            a([Component.text('Install')], href: '#install'),
            a(
              [Component.text('GitHub')],
              href: githubUrl,
              target: Target.blank,
              attributes: {'rel': 'noopener noreferrer'},
            ),
          ], classes: 'site-nav'),
        ], classes: 'header-inner wrap'),
      ], classes: 'site-header'),
      const Home(),
      footer([
        div([
          p([
            Component.text(
              'Morph is a Weird Brains project, released under the BSD 3-Clause License.',
            ),
          ]),
          div([
            a(
              [Component.text('GitHub')],
              href: githubUrl,
              target: Target.blank,
              attributes: {'rel': 'noopener noreferrer'},
            ),
            a(
              [Component.text('TAUR packages')],
              href: taurUrl,
              target: Target.blank,
              attributes: {'rel': 'noopener noreferrer'},
            ),
            a([Component.text(siteUrl.replaceFirst('https://', ''))], href: '$siteUrl/'),
          ], classes: 'footer-links'),
        ], classes: 'footer-inner wrap'),
      ], classes: 'site-footer'),
    ]);
  }
}
