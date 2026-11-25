import 'package:jaspr/dom.dart';
import 'package:jaspr/jaspr.dart';

import '../components/layout.dart';
import '../site.dart';

/// Files on the v0.2.3 GitHub release (name, what it is, size).
const _linux = [
  ('morph-$latestVersion-x86_64-unknown-linux-gnu.tar.gz', 'Tarball', '16.2 MB'),
  ('morph-$latestVersion-x86_64-unknown-linux-gnu.zip', 'Zip', '16.2 MB'),
  ('morph_$latestVersion-1_amd64.deb', 'Debian package, with desktop entry', '11.5 MB'),
  ('morph-standalone_$latestVersion-1_amd64.deb', 'Debian package, binary only', '11.5 MB'),
  ('morph-$latestVersion-1.x86_64.rpm', 'RPM package, with desktop entry', '16.1 MB'),
  ('morph-standalone-$latestVersion-1.x86_64.rpm', 'RPM package, binary only', '16.1 MB'),
];
const _mac = [
  ('morph-$latestVersion-universal-apple-darwin.dmg', 'Disk image', '22.6 MB'),
  ('morph-$latestVersion-universal-apple-darwin.zip', 'Zip with Morph.app', '21.2 MB'),
];
const _windows = [
  ('morph-$latestVersion-x86_64-pc-windows-msvc.msi', 'Installer', '9.6 MB'),
  ('morph-$latestVersion-x86_64-pc-windows-msvc.zip', 'Zip, no installation', '11.0 MB'),
];
const _android = [
  ('morph-$latestVersion-android-arm64.apk', 'arm64 release APK, signed with a test key', '22.7 MB'),
  ('morph-$latestVersion-android-debug.apk', 'Debug APK', '128.8 MB'),
];

/// Files on the v0.3.0-preview.3 GitHub pre-release (name, what it is, size).
const _preview = [
  ('morph-$previewVersion-x86_64-unknown-linux-gnu.tar.gz', 'Linux tarball', '20.8 MB'),
  ('morph-$previewVersion-x86_64-unknown-linux-gnu.zip', 'Linux zip', '20.8 MB'),
  ('morph_0.3.0.preview.3-1_amd64.deb', 'Debian package, with desktop entry', '14.9 MB'),
  ('morph-standalone_0.3.0.preview.3-1_amd64.deb', 'Debian package, binary only', '14.9 MB'),
  ('morph-0.3.0.preview.3-1.x86_64.rpm', 'RPM package, with desktop entry', '20.7 MB'),
  ('morph-standalone-0.3.0.preview.3-1.x86_64.rpm', 'RPM package, binary only', '20.7 MB'),
  ('morph-$previewVersion-universal-apple-darwin.dmg', 'macOS disk image', '36.0 MB'),
  ('morph-$previewVersion-universal-apple-darwin.zip', 'macOS zip with Morph.app', '31.7 MB'),
  ('morph-$previewVersion-x86_64-pc-windows-msvc.msi', 'Windows installer', '13.4 MB'),
  ('morph-$previewVersion-x86_64-pc-windows-msvc.zip', 'Windows zip, no installation', '15.6 MB'),
  ('morph-$previewVersion-android-arm64.apk', 'Android arm64 release APK, signed with a test key', '23.4 MB'),
  ('morph-$previewVersion-android-debug.apk', 'Android debug APK', '130.9 MB'),
];

class DownloadPage extends StatelessComponent {
  const DownloadPage({super.key});

  @override
  Component build(BuildContext context) {
    return Component.fragment([
      pageHero(
        eyebrow: 'Download',
        title: 'Get Morph $latestVersion',
        lede:
            'Released $latestDate. Every file is on the GitHub release, listed in SHA256SUMS so you can check it '
            'before you open it.',
      ),
      section([
        div([
          div([
            a([Component.text('Preview: $previewVersion')], href: '#preview'),
            extLink('Release notes', latestReleaseUrl),
            extLink('SHA256SUMS', assetUrl('SHA256SUMS')),
            extLink('Changelog', changelogUrl),
            extLink('All releases', releasesUrl),
          ], classes: 'pill-links'),
          _platform('linux', 'Linux', 'x86_64. Needs WebKitGTK 4.1, GTK 3 and a Vulkan driver.', _linux, [
            h3([Component.text('Arch Linux')]),
            p([
              Component.text('The prebuilt '),
              code([Component.text('morph-bin')]),
              Component.text(' package ($latestVersion-1) is on the '),
              a([Component.text('Weird Brains')], href: weirdBrainsUrl),
              Component.text(' '),
              extLink('TAUR server', taurUrl),
              Component.text(':'),
            ]),
            pre([
              code([Component.text('yay --aururl https://pkg.taur.weirdbrains.com -S morph-bin')]),
            ]),
            h3([Component.text('Nix')]),
            p([Component.text('The flake in the repository packages the release tarball:')]),
            pre([
              code([Component.text('nix run github:WeirdBrains/morph')]),
            ]),
          ]),
          _platform('macos', 'macOS', 'Universal (Apple silicon and Intel), macOS 11 or later.', _mac, [
            p([
              Component.text(
                'The app is ad-hoc signed, not notarized, so macOS warns before the first start. ',
              ),
              extLink('How to open it', unsignedDocsUrl),
              Component.text('.'),
            ], classes: 'callout'),
          ]),
          _platform(
            'windows',
            'Windows',
            'x64, Windows 10 21H2 or later, or Windows 11. Uses the WebView2 runtime.',
            _windows,
            [
              p([
                Component.text('The files are not code-signed yet, so SmartScreen asks before the first start. '),
                extLink('How to run it', unsignedDocsUrl),
                Component.text('.'),
              ], classes: 'callout'),
            ],
          ),
          _platform('android', 'Android', 'Sideload the APK. Store and F-Droid builds are planned.', _android, []),
          section(
            [
              h2([Component.text('iOS')]),
              p([
                Component.text(
                  'iOS builds run in CI, but there is no public iOS build yet. TestFlight and the App Store are planned.',
                ),
              ]),
            ],
            classes: 'platform',
            id: 'ios',
          ),
          section(
            [
              h2([Component.text('Preview: $previewVersion')]),
              p([
                span([Component.text('Preview')], classes: 'tag'),
                Component.text(
                  ' A first look at v0.3, published as a GitHub pre-release. This preview lets companions run on a Ymir GPU pool, '
                  'with an automatic fallback to the model on this device. Morph $latestVersion stays the stable, '
                  'recommended release; install that unless you want to try the preview. Previews may change, and '
                  'linking devices in v0.3 needs v0.3 on both.',
                ),
              ], classes: 'callout'),
              ul([
                for (final (file, what, size) in _preview)
                  li([
                    a([Component.text(file)], href: previewAssetUrl(file), classes: 'file'),
                    span([Component.text('$what · $size')], classes: 'file-meta'),
                  ]),
              ], classes: 'files'),
              p([
                extLink('Preview release notes and checksums', previewReleaseUrl),
                Component.text('.'),
              ]),
              p([
                Component.text('On Arch Linux, the '),
                code([Component.text('morph-bin')]),
                Component.text(' package at version 0.3.0preview3-1 is a preview too, on the '),
                a([Component.text('Weird Brains')], href: weirdBrainsUrl),
                Component.text(' '),
                extLink('TAUR server', taurUrl),
                Component.text('. The command above installs whichever version TAUR lists as newest, so check the version it shows before you confirm.'),
              ]),
            ],
            classes: 'platform',
            id: 'preview',
          ),
          section(
            [
              h2([Component.text('Build from source')]),
              p([
                Component.text('You need Rust 1.99 or newer, and Flutter 3.50 (Dart 3.14) for the phone app in '),
                code([Component.text('app/')]),
                Component.text('. On Arch, the desktop shell needs these libraries:'),
              ]),
              pre([
                code([
                  Component.text(
                    'sudo pacman -S --needed webkit2gtk-4.1 gtk3 libxkbcommon-x11 libxcb vulkan-icd-loader \\\n'
                    '  fontconfig freetype2 clang cmake pkgconf',
                  ),
                ]),
              ]),
              p([Component.text('Then build and run the desktop shell, with its data in a separate folder:')]),
              pre([
                code([
                  Component.text(
                    'git clone https://github.com/WeirdBrains/morph.git\n'
                    'cd morph\n'
                    'MORPH_HOME=\$(mktemp -d) cargo run -p morph-desktop --release -- example.com',
                  ),
                ]),
              ]),
              p([
                Component.text('On Debian or Ubuntu, the build packages are listed in '),
                code([Component.text('DESKTOP_DEPS')]),
                Component.text(' in the CI workflow. See the '),
                extLink('README', '$githubUrl#build'),
                Component.text(' for the rest.'),
              ]),
            ],
            classes: 'platform',
            id: 'source',
          ),
        ], classes: 'wrap'),
      ], classes: 'band'),
    ]);
  }
}

Component _platform(String id, String name, String about, List<(String, String, String)> files, List<Component> extra) {
  return section(
    [
      h2([Component.text(name)]),
      p([Component.text(about)]),
      ul([
        for (final (file, what, size) in files)
          li([
            a([Component.text(file)], href: assetUrl(file), classes: 'file'),
            span([Component.text('$what · $size')], classes: 'file-meta'),
          ]),
      ], classes: 'files'),
      ...extra,
    ],
    classes: 'platform',
    id: id,
  );
}
