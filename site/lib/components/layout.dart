import 'package:jaspr/dom.dart';
import 'package:jaspr/jaspr.dart';

import '../site.dart';

const _nav = [
  ('/features', 'Features'),
  ('/roadmap', 'Roadmap'),
  ('/download', 'Download'),
];

const _external = {'rel': 'noopener noreferrer'};

/// Header, head metadata, main and footer around a page.
class Layout extends StatelessComponent {
  const Layout({
    required this.path,
    required this.title,
    required this.description,
    required this.child,
    super.key,
  });

  /// The route, such as `/` or `/features`; it is also the active nav item.
  final String path;
  final String title;
  final String description;
  final Component child;

  @override
  Component build(BuildContext context) {
    final url = path == '/' ? '$siteUrl/' : '$siteUrl$path/';
    return Component.fragment([
      Document.head(
        title: title,
        meta: {
          'description': description,
          'twitter:card': 'summary',
          'twitter:title': title,
          'twitter:description': description,
          'twitter:image': ogImageUrl,
          'twitter:image:alt': ogImageAlt,
        },
        children: [
          meta(attributes: {'property': 'og:type'}, content: 'website'),
          meta(attributes: {'property': 'og:site_name'}, content: siteTitle),
          meta(attributes: {'property': 'og:title'}, content: title),
          meta(attributes: {'property': 'og:description'}, content: description),
          meta(attributes: {'property': 'og:url'}, content: url),
          meta(attributes: {'property': 'og:image'}, content: ogImageUrl),
          meta(attributes: {'property': 'og:image:alt'}, content: ogImageAlt),
          link(href: url, rel: 'canonical', attributes: {'id': 'canonical'}),
        ],
      ),
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
            for (final (href, label) in _nav)
              a(
                [Component.text(label)],
                href: href,
                classes: path == href ? 'active' : null,
                attributes: path == href ? {'aria-current': 'page'} : null,
              ),
            a([Component.text('Weirdnet')], href: weirdnetUrl, target: Target.blank, attributes: _external),
            a([Component.text('GitHub')], href: githubUrl, target: Target.blank, attributes: _external),
          ], classes: 'site-nav'),
        ], classes: 'header-inner wrap'),
      ], classes: 'site-header'),
      Component.element(tag: 'main', id: 'content', children: [child]),
      footer([
        div([
          p([
            Component.text('Morph is a Weird Brains project, released under the BSD 3-Clause License. '),
            Component.text('Built on '),
            a([Component.text('Weirdnet')], href: weirdnetUrl),
            Component.text('.'),
          ]),
          div([
            a([Component.text('Releases')], href: releasesUrl, target: Target.blank, attributes: _external),
            a([Component.text('Changelog')], href: changelogUrl, target: Target.blank, attributes: _external),
            a([Component.text('TAUR packages')], href: taurUrl, target: Target.blank, attributes: _external),
            a([Component.text('GitHub')], href: githubUrl, target: Target.blank, attributes: _external),
          ], classes: 'footer-links'),
        ], classes: 'footer-inner wrap'),
      ], classes: 'site-footer'),
    ]);
  }
}

/// A page title block.
Component pageHero({required String eyebrow, required String title, required String lede}) {
  return section([
    div([
      p([Component.text(eyebrow)], classes: 'status'),
      h1([Component.text(title)]),
      p([Component.text(lede)], classes: 'lede'),
    ], classes: 'hero-inner'),
  ], classes: 'hero page-hero wrap');
}

enum Status { shipped, inProgress, planned }

/// A status label: Shipped, In progress or Planned.
Component statusTag(Status s) => switch (s) {
  Status.shipped => span([Component.text('Shipped')], classes: 'tag tag-shipped'),
  Status.inProgress => span([Component.text('In progress')], classes: 'tag tag-progress'),
  Status.planned => span([Component.text('Planned')], classes: 'tag tag-planned'),
};

/// A screenshot or illustration with a caption.
Component shot(String src, String alt, String caption, {bool concept = false, bool lazy = true}) {
  return figure([
    img(
      src: 'images/$src',
      alt: alt,
      attributes: {if (lazy) 'loading': 'lazy', 'decoding': 'async'},
    ),
    figcaption([
      if (concept) span([Component.text('Concept')], classes: 'tag tag-concept'),
      Component.text(caption),
    ]),
  ], classes: concept ? 'shot concept' : 'shot');
}

/// An external link that opens in a new tab.
Component extLink(String label, String href) =>
    a([Component.text(label)], href: href, target: Target.blank, attributes: _external);
