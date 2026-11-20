import 'package:jaspr/dom.dart';
import 'package:jaspr/jaspr.dart';

import '../components/layout.dart';
import '../site.dart';

/// The front page. Old one-page anchors (#about, #features, #built,
/// #platforms, #install) still land on a matching section.
class Home extends StatelessComponent {
  const Home({super.key});

  @override
  Component build(BuildContext context) {
    return Component.fragment([
      section(
        [
          div([
            p([Component.text('Version $latestVersion · Linux, macOS, Windows and Android')], classes: 'status'),
            h1([Component.text(siteTagline)]),
            p([
              Component.text(
                'The browser is one part of the app, next to local AI, projects, notes, and a peer-to-peer web.',
              ),
            ], classes: 'lede'),
            p([Component.text('Morph is built so your data stays yours, and nothing leaks.')], classes: 'support'),
            div([
              a([Component.text('Download Morph $latestVersion')], href: '/download', classes: 'button button-primary'),
              a([Component.text('See the features')], href: '/features', classes: 'button button-ghost'),
            ], classes: 'actions'),
          ], classes: 'hero-inner'),
          shot(
            'screenshot-main-tabs.webp',
            'Morph on Linux with four tabs open and a tide pool field guide page loaded.',
            'Morph $latestVersion on Linux.',
            lazy: false,
          ),
        ],
        classes: 'hero hero-split wrap',
        id: 'top',
      ),
      section(
        [
          div([
            h2([Component.text('What Morph is')]),
            div([
              p([
                Component.text(
                  'Morph is a private, decentralized browser with local AI built in. It is meant to be a research '
                  'assistant, a rubber duck, and a journaler: a place for both short-term and long-term work on the web.',
                ),
              ]),
              p([
                Component.text(
                  'The desktop app is a host for swappable components. Browsing is one of them, next to your '
                  'bookmarks, history, identity and settings today, and chat, notes and projects as they arrive.',
                ),
              ]),
            ], classes: 'prose'),
          ], classes: 'wrap'),
        ],
        classes: 'band',
        id: 'about',
      ),
      section(
        [
          div([
            h2([Component.text('A look at Morph')]),
            p([
              Component.text('Real screenshots of Morph 0.2, taken with a demo profile.'),
            ], classes: 'intro'),
            div([
              shot(
                'screenshot-light-side.webp',
                'Morph in the light theme with tabs in a list on the left side.',
                'Light theme, tabs on the left.',
              ),
              shot(
                'screenshot-bookmarks.webp',
                'The bookmarks panel beside a page, with tag chips and folders.',
                'Bookmarks with folders and tags.',
              ),
              shot(
                'screenshot-link-device.webp',
                'The identity panel showing a QR code for linking a new device.',
                'Linking a device with a QR code.',
              ),
            ], classes: 'gallery'),
            p([
              a([Component.text('All features →')], href: '/features', classes: 'more'),
            ]),
          ], classes: 'wrap'),
        ],
        classes: 'band',
        id: 'features',
      ),
      section(
        [
          div([
            h2([Component.text('Get Morph $latestVersion')], id: 'install'),
            p([
              Component.text('Released $latestDate. Downloads are on '),
              extLink('GitHub', latestReleaseUrl),
              Component.text(', with SHA256SUMS to check them.'),
            ], classes: 'intro'),
            Component.element(
              tag: 'dl',
              classes: 'facts',
              children: [
                Component.element(tag: 'dt', children: [Component.text('Linux')]),
                Component.element(
                  tag: 'dd',
                  children: [
                    Component.text('x86_64: tarball, zip, deb, rpm, Nix flake, and morph-bin for Arch on the '),
                    a([Component.text('Weird Brains')], href: weirdBrainsUrl),
                    Component.text(' TAUR server.'),
                  ],
                ),
                Component.element(tag: 'dt', children: [Component.text('macOS')]),
                Component.element(
                  tag: 'dd',
                  children: [
                    Component.text('Universal (Apple silicon and Intel), dmg or zip. Not notarized yet.'),
                  ],
                ),
                Component.element(tag: 'dt', children: [Component.text('Windows')]),
                Component.element(tag: 'dd', children: [Component.text('x64, msi or zip. Not code-signed yet.')]),
                Component.element(tag: 'dt', children: [Component.text('Android and iOS')]),
                Component.element(
                  tag: 'dd',
                  children: [
                    Component.text('An arm64 APK to sideload. iOS builds run in CI; there is no public iOS build yet.'),
                  ],
                ),
              ],
            ),
            div([
              a([Component.text('Download')], href: '/download', classes: 'button button-primary'),
            ], classes: 'actions'),
          ], classes: 'wrap'),
        ],
        classes: 'band',
        id: 'platforms',
      ),
      section(
        [
          div([
            h2([Component.text("How it's built")]),
            div([
              p([
                Component.text(
                  'A Rust 2024 core owns identity, data, networking and AI. The shells draw the screen and route '
                  'input; they never hold keys or business logic.',
                ),
              ]),
              p([
                Component.text(
                  'On Linux, macOS and Windows the shell is Rust on GPUI, Zed’s UI framework, and runs natively on '
                  'Wayland or X11 on Linux. On Android and iOS it is a Flutter app over the same Rust crates. Pages use '
                  'the system web view everywhere, and networking, keys and sync come from ',
                ),
                a([Component.text('Weirdnet')], href: weirdnetUrl),
                Component.text('.'),
              ]),
            ], classes: 'prose'),
          ], classes: 'wrap'),
        ],
        classes: 'band',
        id: 'built',
      ),
      section(
        [
          div([
            h2([Component.text('Coming soon')]),
            p([
              Component.text('What the next releases bring. Android and iOS move forward with each one.'),
            ], classes: 'intro'),
            div([
              _milestone(
                'v0.3',
                'Peer-to-peer, plus a password saver',
                'Peers and relays, Grindvakt dynamic DNS, real peer content at weirdnet-morph://, and saved passwords.',
                Status.inProgress,
              ),
              _milestone(
                'v0.4',
                'Local AI and projects',
                'Local models, projects with code spaces, and hosting them privately or publicly.',
                Status.inProgress,
              ),
              _milestone(
                'v0.5',
                'Extensions',
                'Chrome and Firefox extensions unchanged, each in its own sandbox.',
                Status.planned,
              ),
            ], classes: 'feature-grid'),
            p([
              a([Component.text('The full roadmap →')], href: '/roadmap', classes: 'more'),
            ]),
          ], classes: 'wrap'),
        ],
        classes: 'band',
        id: 'coming-soon',
      ),
    ]);
  }
}

Component _milestone(String version, String title, String body, Status status) {
  return a(
    [
      span([Component.text(version)], classes: 'milestone'),
      h3([Component.text(title)]),
      p([Component.text(body)]),
      statusTag(status),
    ],
    href: '/roadmap#${version.replaceAll('.', '-')}',
    classes: 'card card-link',
  );
}
