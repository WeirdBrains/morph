import 'package:jaspr/dom.dart';
import 'package:jaspr/jaspr.dart';

import '../site.dart';

class Home extends StatelessComponent {
  const Home({super.key});

  @override
  Component build(BuildContext context) {
    return Component.element(
      tag: 'main',
      id: 'content',
      children: [
        section(
          [
            div([
              p([Component.text('Early development')], classes: 'status'),
              h1([Component.text(siteTagline)]),
              p([
                Component.text(
                  'The browser is one part of the app, next to local AI, projects, notes, '
                  'and a peer-to-peer web.',
                ),
              ], classes: 'lede'),
              p([
                Component.text(
                  'Morph is built so your data stays yours, and nothing leaks.',
                ),
              ], classes: 'support'),
              div([
                a([Component.text('Install')], href: '#install', classes: 'button button-primary'),
                a(
                  [Component.text('View the source on GitHub')],
                  href: githubUrl,
                  target: Target.blank,
                  attributes: {'rel': 'noopener noreferrer'},
                  classes: 'button button-ghost',
                ),
              ], classes: 'actions'),
            ], classes: 'hero-inner'),
          ],
          classes: 'hero wrap',
          id: 'top',
        ),
        section(
          [
            div([
              h2([Component.text('What Morph is')]),
              div([
                p([
                  Component.text(
                    'Morph is a private, decentralized browser with local AI built in. '
                    'It is a research assistant, a rubber duck, and a journaler: a place '
                    'for both short-term and long-term work on the web.',
                  ),
                ]),
                p([
                  Component.text(
                    'The desktop shell is a host for swappable components. Browsing is '
                    'one of those components, sitting next to chat, notes, projects, and '
                    'the people you are connected to.',
                  ),
                ]),
                p([
                  Component.text(
                    'It is standards-based, with pages rendered in the system web view on '
                    'every platform. A Rust core owns your identity, data, networking, and '
                    'AI. The UI only draws the screen and handles input.',
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
              h2([Component.text('Features')]),
              p([
                Component.text(
                  'These are the parts Morph is designed around. Some are already in '
                  'progress; others are planned in the project priorities.',
                ),
              ], classes: 'intro'),
              div([
                _feature(
                  'A standards-based browser',
                  'Pages render in the system web view: WebKit on Linux, WKWebView on '
                      'Apple platforms, WebView2 on Windows, and Android WebView. The '
                      'browser is one component of the app, not the whole app.',
                ),
                _feature(
                  'Local AI',
                  'Local models are on by default, with one switch to turn them off. '
                      'Morph starts as a chat partner and rubber duck. Later it becomes a '
                      'Student that can learn from the browsing you opt into, and a '
                      'Researcher that keeps research journals across the web, your peers, '
                      'and your chats.',
                ),
                _feature(
                  'Projects and code spaces',
                  'Projects include templates, notes, a code editor, and code spaces — '
                      'groups of repos and folders. They are Rust first. A project can be '
                      'private (the default) or public, and gated or open. Public projects '
                      'can be served over ordinary HTTPS at your own dynamic DNS name.',
                ),
                _feature(
                  'A peer-to-peer web on Weirdnet',
                  'You get one key identity, shared with Muninn. Peer sites live at ',
                  after: [
                    code([Component.text('weirdnet-morph://')]),
                    Component.text(
                      '. Relays help reach devices that DNS cannot, and Grindvakt '
                      'dynamic DNS can tie a domain name to your browser.',
                    ),
                  ],
                ),
                _feature(
                  'Notes, a journal, chat, and forums',
                  'Write notes and a journal, and talk with the people you are connected '
                      'to. Forums stay Morph-only.',
                ),
                _feature(
                  'Your own look and layout',
                  'Components can be themed and swapped to fit how you work. A later '
                      'drag-and-drop layout mode will let you place components and choose '
                      'what is shown by default.',
                ),
                _feature(
                  'Sandboxed extensions',
                  'Extensions are downloaded and installed only with your permission, '
                      'signed by the author\'s key. Each one runs in its own sandboxed '
                      'process. The first SDK is Rust; other compiled languages can use '
                      'the same gRPC contract.',
                ),
                _feature(
                  'Signed updates, never forced',
                  'Updates come through Salty Gull. They are signed, and they are never '
                      'forced: you opt in to each update, or opt in to all of them.',
                ),
              ], classes: 'feature-grid'),
            ], classes: 'wrap'),
          ],
          classes: 'band',
          id: 'features',
        ),
        section(
          [
            div([
              h2([Component.text("How it's built")]),
              div([
                p([
                  Component.text(
                    'A Rust 2024 core owns identity, data, networking, and AI. The shells '
                    'draw the screen and route input; they do not hold keys or business '
                    'logic. Anything both shells need lives in a Morph crate that can be '
                    'tested without a display.',
                  ),
                ]),
                p([
                  Component.text(
                    'On the desktop, the shell is written in Rust with GPUI, Zed\'s UI '
                    'framework. Linux comes first; macOS and Windows on GPUI are planned. '
                    'On mobile, the shell stays Flutter, on Android, iOS, and F-Droid. Both '
                    'shells sit on the same Rust crates. Pages use the system web view on '
                    'every platform.',
                  ),
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
              h2([Component.text('Platforms and status')]),
              p([
                Component.text(
                  'Morph is in early development. Phase 1 of the GPUI desktop shell is in '
                  'progress, with Linux first.',
                ),
              ], classes: 'intro'),
              Component.element(
                tag: 'dl',
                classes: 'facts',
                children: [
                  Component.element(tag: 'dt', children: [Component.text('Linux desktop')]),
                  Component.element(
                    tag: 'dd',
                    children: [
                      Component.text(
                        'The current focus. The GPUI shell is being built so you can browse, '
                        'use tabs and the address bar, and type in pages and popups.',
                      ),
                    ],
                  ),
                  Component.element(tag: 'dt', children: [Component.text('macOS and Windows')]),
                  Component.element(
                    tag: 'dd',
                    children: [
                      Component.text(
                        'Planned on the same GPUI shell, after Linux is solid. Not started yet.',
                      ),
                    ],
                  ),
                  Component.element(tag: 'dt', children: [Component.text('Android, iOS, and F-Droid')]),
                  Component.element(
                    tag: 'dd',
                    children: [
                      Component.text(
                        'The Flutter mobile shell stays. It shares the same Rust core as desktop.',
                      ),
                    ],
                  ),
                  Component.element(tag: 'dt', children: [Component.text('License')]),
                  Component.element(
                    tag: 'dd',
                    children: [
                      Component.text('BSD 3-Clause. Source is on '),
                      a(
                        [Component.text('GitHub')],
                        href: githubUrl,
                        target: Target.blank,
                        attributes: {'rel': 'noopener noreferrer'},
                      ),
                      Component.text('.'),
                    ],
                  ),
                ],
              ),
            ], classes: 'wrap'),
          ],
          classes: 'band',
          id: 'platforms',
        ),
        section(
          [
            div([
              h2([Component.text('Install')]),
              div([
                h3([Component.text('Arch Linux')]),
                p([
                  Component.text('The '),
                  code([Component.text('morph')]),
                  Component.text(' and '),
                  code([Component.text('morph-bin')]),
                  Component.text(
                    ' packages are coming with the first build. They will be published on ',
                  ),
                  a(
                    [Component.text('the WeirdBrains TAUR server')],
                    href: taurUrl,
                    target: Target.blank,
                    attributes: {'rel': 'noopener noreferrer'},
                  ),
                  Component.text(
                    '. Until that build is ready, install from source using the steps in the repository README.',
                  ),
                ]),
              ], classes: 'install-block'),
              div([
                h3([Component.text('From source')]),
                p([
                  Component.text(
                    'Clone the repository and follow the README for current build steps. '
                    'The GPUI desktop work is happening on ',
                  ),
                  code([Component.text('main')]),
                  Component.text(
                    '. You will need Rust 1.99 or newer. Flutter 3.50 (Dart 3.14) is required for the mobile app, and for the current Flutter shell on ',
                  ),
                  code([Component.text('main')]),
                  Component.text(' until the GPUI shell lands.'),
                ]),
                pre([
                  code([
                    Component.text(
                      'git clone https://github.com/WeirdBrains/morph.git\n'
                      'cd morph',
                    ),
                  ]),
                ]),
                p([
                  Component.text(
                    'On Arch, the GPUI desktop shell will need these system libraries:',
                  ),
                ]),
                pre([
                  code([
                    Component.text(
                      'sudo pacman -S --needed webkit2gtk-4.1 gtk3 libxkbcommon-x11 vulkan-icd-loader',
                    ),
                  ]),
                ]),
                p([
                  Component.text('Once the desktop crate is in the tree, the intended command is:'),
                ]),
                pre([
                  code([Component.text('cargo run -p morph-desktop --release')]),
                ]),
                p([
                  Component.text('Until then, the README covers building the existing Flutter app in '),
                  code([Component.text('app/')]),
                  Component.text('.'),
                ]),
              ], classes: 'install-block'),
            ], classes: 'wrap'),
          ],
          classes: 'band',
          id: 'install',
        ),
      ],
    );
  }
}

Component _feature(String title, String body, {List<Component>? after}) {
  return article([
    h3([Component.text(title)]),
    p([
      Component.text(body),
      ...?after,
    ]),
  ], classes: 'card');
}
