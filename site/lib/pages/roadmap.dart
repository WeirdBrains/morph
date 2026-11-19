import 'package:jaspr/dom.dart';
import 'package:jaspr/jaspr.dart';

import '../components/layout.dart';
import '../site.dart';

/// The roadmap, from PRIORITIES.md ("Roadmap after 0.2.0", decided 2026-11-15) and the
/// status table in docs/handoff.md.
class RoadmapPage extends StatelessComponent {
  const RoadmapPage({super.key});

  @override
  Component build(BuildContext context) {
    return Component.fragment([
      pageHero(
        eyebrow: 'Coming soon',
        title: 'Roadmap',
        lede:
            'Morph $latestVersion shipped on $latestDate. This is what comes next, from the project’s adopted '
            'priorities. Android and iOS are not a milestone of their own: they move forward with every milestone.',
      ),
      section([
        div([
          p([
            statusTag(Status.shipped),
            Component.text(' is in a release. '),
            statusTag(Status.inProgress),
            Component.text(' is being built on main. '),
            statusTag(Status.planned),
            Component.text(
              ' is decided but not started. Illustrations are concepts, not screenshots: no mock-ups of '
              'unreleased screens are published yet.',
            ),
          ], classes: 'legend'),
          _milestone(
            id: 'v0-2',
            version: 'v0.2',
            title: 'The desktop browser, sync and linked devices',
            status: Status.shipped,
            items: [
              (
                'GPUI desktop shell on Linux, macOS and Windows, with the Android app on the same Rust core',
                Status.shipped,
              ),
              ('Bookmarks, history and settings sync between linked devices over Weirdnet', Status.shipped),
              (
                'Settings > Linked devices with revoke; Relays with a discovery switch and manual peers',
                Status.shipped,
              ),
              ('A downloads panel, and downloads that resume behind a login', Status.shipped),
              ('Downloads that resume behind JavaScript logins (0.2.1)', Status.shipped),
              (
                'More than one window: new window, move a tab to a new window, windows and tabs reopen at launch (0.2.1)',
                Status.shipped,
              ),
              ('Switching profiles keeps each profile’s tabs and windows (0.2.2)', Status.shipped),
              ('weirdnet- links call the app over Weirdnet', Status.shipped),
              (
                'Key rotation on revoke, so a revoked device can’t read later sync (weirdnet-sync 0.3)',
                Status.inProgress,
              ),
            ],
            note: 'Released as $latestVersion. Work on 0.2.x fixes continues.',
          ),
          _milestone(
            id: 'v0-3',
            version: 'v0.3',
            title: 'Peer-to-peer, plus the password saver',
            status: Status.planned,
            items: [
              ('Weirdnet peers and relays in Morph', Status.planned),
              ('Grindvakt dynamic DNS, so a domain follows your device', Status.planned),
              ('weirdnet-morph:// resolves real peer content, with the shared scheme registry', Status.planned),
              ('Password saver', Status.planned),
              ('First local AI helpers, moved up from v0.4', Status.inProgress),
            ],
            image: (
              'concept-p2p-web.svg',
              'Concept diagram: a Morph device hosting a site, a relay, and a peer opening a weirdnet-morph link.',
              'Peer sites over Weirdnet.',
            ),
          ),
          _milestone(
            id: 'v0-4',
            version: 'v0.4',
            title: 'Local AI and projects',
            status: Status.planned,
            items: [
              (
                'Local AI: models from Hugging Face, run on your device, on by default with one switch to turn it off',
                Status.planned,
              ),
              (
                'MCP-native: MCP servers as tool sources for local models and agents, each with its own switch and approvals',
                Status.planned,
              ),
              ('Projects with templates, notes and code spaces, Rust first', Status.planned),
              ('Hosting projects, private or public, with access control', Status.planned),
              ('Optional HTTPS for public projects through weirdnet-tls', Status.planned),
            ],
            image: (
              'concept-local-ai.svg',
              'Concept diagram: a web page beside a local model panel with MCP tools.',
              'Local AI beside the page, with MCP tools.',
            ),
          ),
          _milestone(
            id: 'v0-5',
            version: 'v0.5',
            title: 'Extensions',
            status: Status.planned,
            items: [
              ('Chrome and Firefox extensions run unchanged', Status.planned),
              ('The WebExtensions API implemented in Rust, with a thin chrome.* and browser.* shim', Status.planned),
              ('Manifest v2 and v3, .crx and .xpi, with a signature check', Status.planned),
              (
                'Every extension in its own sandboxed process; native extensions too, WASM where native code can’t be downloaded',
                Status.planned,
              ),
              ('The goal: 1Password, Bitwarden and LastPass work unchanged', Status.planned),
            ],
            image: (
              'concept-extensions.svg',
              'Concept diagram: an extension in a sandbox talking to Morph over gRPC.',
              'Extensions in their own sandbox.',
            ),
          ),
          _milestone(
            id: 'later',
            version: 'Later',
            title: 'Updates, people and polish',
            status: Status.planned,
            items: [
              ('Signed updates through Salty Gull, announced over Weirdnet, never forced', Status.planned),
              ('Chat with peers, forums, notes and a journal', Status.planned),
              ('A drag-and-drop layout mode, and theming for every component', Status.planned),
              ('The Student and Researcher layers of the local AI, opt-in', Status.planned),
            ],
            image: (
              'concept-updates.svg',
              'Concept diagram: a signed release passed through relays to Morph, which asks before updating.',
              'Signed updates that ask first.',
            ),
          ),
          h2([Component.text('Platforms')], id: 'platforms'),
          div([
            table([
              thead([
                tr([
                  th([Component.text('Platform')]),
                  th([Component.text('Status')]),
                  th([Component.text('Notes')]),
                ]),
              ]),
              tbody([
                _row(
                  'Linux x86_64',
                  Status.shipped,
                  'Tarball, zip, deb, rpm, Nix flake and morph-bin on TAUR. ARM64 waits for an ARM64 build runner.',
                ),
                _row(
                  'macOS (universal)',
                  Status.shipped,
                  'dmg and zip, ad-hoc signed, not notarized. Not there yet: default browser, links from other apps.',
                ),
                _row(
                  'Windows x64',
                  Status.shipped,
                  'msi and zip, not code-signed. Not there yet: default browser, a crashed-page panel. ARM64 is not built yet.',
                ),
                _row('Android', Status.shipped, 'arm64 APK to sideload. F-Droid and Google Play are planned channels.'),
                _row(
                  'iOS',
                  Status.inProgress,
                  'Built in CI with every release; TestFlight and the App Store are planned channels.',
                ),
              ]),
            ]),
          ], classes: 'table-wrap'),
          p([
            Component.text(
              'Android and iOS move forward with every milestone above, on the same Rust core as the desktop.',
            ),
          ], classes: 'intro'),
          p([
            Component.text('Sources: '),
            extLink('PRIORITIES.md', '$githubUrl/blob/main/PRIORITIES.md'),
            Component.text(', '),
            extLink('the handoff status table', '$githubUrl/blob/main/docs/handoff.md'),
            Component.text(' and '),
            extLink('release.toml', '$githubUrl/blob/main/release.toml'),
            Component.text('.'),
          ], classes: 'sources'),
        ], classes: 'wrap'),
      ], classes: 'band'),
    ]);
  }
}

Component _milestone({
  required String id,
  required String version,
  required String title,
  required Status status,
  required List<(String, Status)> items,
  (String, String, String)? image,
  String note = '',
}) {
  return section(
    [
      div([
        p([
          span([Component.text(version)], classes: 'milestone'),
          statusTag(status),
        ], classes: 'meta'),
        h2([Component.text(title)]),
        ul([
          for (final (text, s) in items) li([statusTag(s), Component.text(' $text')]),
        ], classes: 'items'),
        if (note.isNotEmpty) p([Component.text(note)], classes: 'note'),
      ], classes: 'milestone-text'),
      if (image != null) shot(image.$1, image.$2, image.$3, concept: true),
    ],
    classes: 'card roadmap-step',
    id: id,
  );
}

Component _row(String platform, Status s, String notes) => tr([
  td([Component.text(platform)]),
  td([statusTag(s)]),
  td([Component.text(notes)]),
]);
